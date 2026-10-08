#!/usr/bin/env python3
"""Exercise a built web image over HTTP (Docker and Python's standard library only)."""

import gzip
import re
import subprocess
import sys
import time
import urllib.error
import urllib.request


def docker(*args):
    return subprocess.check_output(["docker", *args], text=True).strip()


def main():
    image = sys.argv[1] if len(sys.argv) > 1 else "photocraft-web:local"
    container = docker(
        "run", "--detach", "--rm", "--read-only", "--tmpfs", "/tmp",
        "--publish", "127.0.0.1::8080", image,
    )
    try:
        port = docker("port", container, "8080/tcp").rsplit(":", 1)[1]
        base = f"http://127.0.0.1:{port}/"
        # Ignore the developer machine's HTTP proxy for loopback checks.
        client = urllib.request.build_opener(urllib.request.ProxyHandler({}))

        def get(path, encoding="identity"):
            request = urllib.request.Request(base + path, headers={"Accept-Encoding": encoding})
            return client.open(request, timeout=10)

        for attempt in range(30):
            try:
                with get("healthz") as response:
                    assert response.read() == b"ok\n"
                break
            except (urllib.error.URLError, TimeoutError):
                if attempt == 29:
                    raise
                time.sleep(1)

        docker("exec", container, "nginx", "-t")
        assert docker("exec", container, "id", "-u") != "0", "server must run as non-root"
        with get("") as response:
            html = response.read()
            assert response.headers.get_content_type() == "text/html"
            assert response.headers["Cache-Control"] == "no-cache"
            assert b"photocraft_canvas" in html
        with get("index.html", "gzip") as response:
            assert response.headers["Content-Encoding"] == "gzip"
            assert gzip.decompress(response.read()) == html

        assets = set(re.findall(r'''(?:src|href)=["']\./([^"']+\.(?:wasm|js))["']''', html.decode()))
        assert any(path.endswith(".wasm") for path in assets), "no Wasm preload in generated HTML"
        assert any(path.endswith(".js") for path in assets), "no generated loader in HTML"
        for path in sorted(assets):
            expected = "application/wasm" if path.endswith(".wasm") else "text/javascript"
            with get(path) as response:
                raw = response.read()
                assert response.headers.get_content_type() == expected
                assert response.headers["Cache-Control"] == "public, max-age=31536000, immutable"
                assert response.headers["X-Content-Type-Options"] == "nosniff"
                if path.endswith(".wasm"):
                    assert raw.startswith(b"\x00asm"), "invalid WebAssembly header"
            with get(path, "gzip") as response:
                assert response.headers["Content-Encoding"] == "gzip"
                assert "Accept-Encoding" in response.headers["Vary"]
                assert gzip.decompress(response.read()) == raw
            print(f"OK: {path} (MIME, caching, gzip)")

        for path in ("missing.wasm", "missing.js", "missing-page"):
            try:
                get(path)
                raise AssertionError(f"{path} should return 404")
            except urllib.error.HTTPError as error:
                assert error.code == 404
                assert "immutable" not in error.headers.get("Cache-Control", "")
                error.close()
        print("OK: health, non-root/read-only runtime, HTML revalidation, missing-file 404s")
    except Exception:
        print(docker("logs", container), file=sys.stderr)
        raise
    finally:
        subprocess.run(["docker", "stop", container], check=True, stdout=subprocess.DEVNULL)


if __name__ == "__main__":
    main()
