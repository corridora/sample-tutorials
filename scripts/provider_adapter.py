#!/usr/bin/env python3
"""Fastly HTTPS NDJSON logs and scoped historical stats → OTLP/HTTP JSON."""
import argparse, hashlib, hmac, json, os, sqlite3, time, urllib.request
from pathlib import Path
from http.server import BaseHTTPRequestHandler, HTTPServer

def attributes(d):return [{'key':k,'value':{'stringValue':str(v)}} for k,v in d.items()]
def resource():return {'attributes':attributes({'service.name':'sample-tutorials-fastly-adapter','service.namespace':'corridora-samples.tutorials','cloud.provider':'fastly','cloud.resource_id':os.getenv('FASTLY_SERVICE_ID',''),'deployment.environment.name':os.getenv('DEPLOYMENT_ENVIRONMENT','development')})}
def convert_logs(lines):
 out=[]
 for line in lines.splitlines():
  if not line.strip():continue
  v=json.loads(line)
  # Compute log endpoint sends structured request logs; optional native fields preserved.
  record={'timeUnixNano':str(int(v.get('timestamp',time.time())*1e9)),'severityNumber':9,'severityText':'INFO','body':{'stringValue':v.get('event','fastly.edge.request')},'attributes':attributes({'event.id':hashlib.sha256(line.encode()).hexdigest()}|{k:v[k] for k in ['status','cache','pop','service_id','service_version','path','duration_ms'] if k in v})}
  for k in ['traceId','spanId']:
   if k in v:record[k]=v[k]
  out.append(record)
 return {'resourceLogs':[{'resource':resource(),'scopeLogs':[{'scope':{'name':'fastly.https.adapter'},'logRecords':out}]}]}
def convert_stats(data):
 points=[]
 rows=data.get('data',[]) if isinstance(data,dict) else data
 for row in rows:
  timestamp=row.get('start_time',int(time.time()));values=row.get('aggregated',row)
  for name in ['requests','compute_requests','resp_body_bytes','errors','compute_execution_time_ms']:
   if name in values:points.append({'name':'fastly.'+name,'gauge':{'dataPoints':[{'timeUnixNano':str(int(timestamp)*10**9),'asDouble':float(values[name])}]}})
 return {'resourceMetrics':[{'resource':resource(),'scopeMetrics':[{'scope':{'name':'fastly.stats.adapter'},'metrics':points}]}]}
def send(signal,payload):
 base=os.environ['CORRIDORA_OTLP_ENDPOINT'].rstrip('/');key=os.environ['CORRIDORA_INGEST_KEY']
 req=urllib.request.Request(base+'/v1/'+signal,data=json.dumps(payload).encode(),headers={'Content-Type':'application/json','X-CORRIDORA-KEY':key})
 with urllib.request.urlopen(req,timeout=5) as r:return r.status
class Handler(BaseHTTPRequestHandler):
 def do_POST(self):
  expected=os.environ.get('FASTLY_LOG_ADAPTER_TOKEN','');auth=self.headers.get('Authorization','')
  if not expected or not hmac.compare_digest(auth,'Bearer '+expected):self.send_error(401);return
  n=int(self.headers.get('Content-Length','0'))
  if n>256*1024:self.send_error(413);return
  try:
   lines=list(dict.fromkeys(self.rfile.read(n).decode().splitlines()));db=ledger();fresh=[x for x in lines if x.strip() and not db.execute('SELECT 1 FROM sent WHERE id=?',(hashlib.sha256(x.encode()).hexdigest(),)).fetchone()]
   if fresh:
    payload=convert_logs('\n'.join(fresh));batch=hashlib.sha256('\n'.join(fresh).encode()).hexdigest()
    db.execute('INSERT OR REPLACE INTO pending VALUES (?,?,?)',(batch,json.dumps(payload),int(time.time())));db.execute('DELETE FROM pending WHERE id NOT IN (SELECT id FROM pending ORDER BY at DESC LIMIT 32)');db.commit()
    send('logs',payload)
    for line in fresh:db.execute('INSERT OR IGNORE INTO sent VALUES (?,?)',(hashlib.sha256(line.encode()).hexdigest(),int(time.time())))
    db.execute('DELETE FROM pending WHERE id=?',(batch,));db.commit()
   db.close()
  except Exception:self.send_error(503,'Invalid batch or export failed; bounded batch retained for replay');return
  self.send_response(204);self.end_headers()
 def log_message(self,*args):pass
def ledger():
 path=Path(os.getenv('ADAPTER_STATE','.sample/adapter.sqlite'));path.parent.mkdir(mode=0o700,parents=True,exist_ok=True)
 db=sqlite3.connect(path);os.chmod(path,0o600)
 db.execute('CREATE TABLE IF NOT EXISTS sent(id TEXT PRIMARY KEY,at INTEGER)');db.execute('CREATE TABLE IF NOT EXISTS pending(id TEXT PRIMARY KEY,body TEXT,at INTEGER)')
 db.execute('DELETE FROM sent WHERE at<?',(int(time.time())-86400,));db.execute('DELETE FROM pending WHERE at<?',(int(time.time())-86400,));db.execute('DELETE FROM pending WHERE id NOT IN (SELECT id FROM pending ORDER BY at DESC LIMIT 32)');db.execute('DELETE FROM sent WHERE id NOT IN (SELECT id FROM sent ORDER BY at DESC LIMIT 10000)');db.commit();return db
if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('mode',choices=['logs','stats','replay']);p.add_argument('--port',type=int,default=4319);a=p.parse_args()
 if a.mode=='logs':HTTPServer(('127.0.0.1',a.port),Handler).serve_forever()
 elif a.mode=='replay':
  db=ledger()
  for ident,body,_ in db.execute('SELECT * FROM pending ORDER BY at LIMIT 32').fetchall():
   payload=json.loads(body);send('logs',payload)
   for record in payload['resourceLogs'][0]['scopeLogs'][0]['logRecords']:
    event_id=next(x['value']['stringValue'] for x in record['attributes'] if x['key']=='event.id');db.execute('INSERT OR IGNORE INTO sent VALUES (?,?)',(event_id,int(time.time())))
   db.execute('DELETE FROM pending WHERE id=?',(ident,));db.commit()
  db.close()
 else:
  db=ledger();end=int(time.time())//60*60;committed=db.execute('SELECT at FROM sent WHERE id=?',('stats-checkpoint',)).fetchone();start=max(end-900,committed[0] if committed else end-900)
  if start>=end:print('No new stats interval');raise SystemExit(0)
  sid=os.environ['FASTLY_SERVICE_ID'];q=f'https://api.fastly.com/stats/service/{sid}?from={start}&to={end}&by=minute'
  req=urllib.request.Request(q,headers={'Fastly-Key':os.environ['FASTLY_STATS_TOKEN']})
  with urllib.request.urlopen(req,timeout=10) as r:data=json.load(r)
  print('OTLP stats status',send('metrics',convert_stats(data)));db.execute('INSERT OR REPLACE INTO sent VALUES (?,?)',('stats-checkpoint',end));db.commit();db.close()
