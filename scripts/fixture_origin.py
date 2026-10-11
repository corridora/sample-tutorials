#!/usr/bin/env python3
"""Optional standalone controlled origin; put behind your existing TLS proxy."""
import json,time
from http.server import BaseHTTPRequestHandler,HTTPServer
from urllib.parse import urlsplit,parse_qs
class Handler(BaseHTTPRequestHandler):
 def do_GET(self):
  u=urlsplit(self.path)
  if u.path!='/api/fixture':self.send_error(404);return
  if parse_qs(u.query).get('slow')==['1']:time.sleep(1.5)
  b=json.dumps({'fictional':True,'holdings':[{'symbol':'DEMO','quantity':2}],'valuation':85}).encode();self.send_response(200);self.send_header('Content-Type','application/json');self.send_header('Cache-Control','public, max-age=60');self.send_header('Content-Length',str(len(b)));self.end_headers();self.wfile.write(b)
 def log_message(self,*args):pass
HTTPServer(('127.0.0.1',4206),Handler).serve_forever()
