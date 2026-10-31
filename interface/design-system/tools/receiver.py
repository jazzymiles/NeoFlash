"""Local receiver for the NeoFlash Export plugin.

POST /<relative path> with the raw file body -> saved under ROOT (paths outside ROOT are refused).
Usage: python3 tools/receiver.py [ROOT=design-system folder] [PORT=8765]
"""
import os, sys
from http.server import BaseHTTPRequestHandler, HTTPServer
from urllib.parse import unquote

ROOT = os.path.abspath(sys.argv[1]) if len(sys.argv) > 1 else os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
PORT = int(sys.argv[2]) if len(sys.argv) > 2 else 8765


class H(BaseHTTPRequestHandler):
    def _cors(self):
        self.send_header('Access-Control-Allow-Origin', '*')
        self.send_header('Access-Control-Allow-Methods', 'POST, OPTIONS, GET')
        self.send_header('Access-Control-Allow-Headers', '*')
        self.send_header('Access-Control-Allow-Private-Network', 'true')

    def do_OPTIONS(self):
        self.send_response(204); self._cors(); self.end_headers()

    def do_GET(self):
        self.send_response(200); self._cors(); self.end_headers(); self.wfile.write(b'ok')

    def do_POST(self):
        rel = unquote(self.path.lstrip('/'))
        path = os.path.normpath(os.path.join(ROOT, rel))
        if not path.startswith(os.path.normpath(ROOT) + os.sep):
            self.send_response(400); self._cors(); self.end_headers(); return
        n = int(self.headers.get('Content-Length', 0))
        data = self.rfile.read(n)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, 'wb') as f:
            f.write(data)
        print(f'{len(data):>9}  {rel}', flush=True)
        self.send_response(200); self._cors(); self.end_headers(); self.wfile.write(str(len(data)).encode())

    def log_message(self, *a):
        pass


print(f'Receiving into {ROOT} on http://localhost:{PORT}  (Ctrl+C to stop)', flush=True)
HTTPServer(('127.0.0.1', PORT), H).serve_forever()
