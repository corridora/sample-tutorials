"""Standalone configuration, local execution and Fastly API deployment."""
import argparse, base64, getpass, hashlib, json, os, re, secrets, shutil, subprocess, sys, tarfile, time
import urllib.request, urllib.parse, urllib.error
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
STATE=ROOT/'.sample'
SECRET={'CORRIDORA_INGEST_KEY','DEMO_CONTROL_TOKEN','DOWNSTREAM_AUTH_TOKEN','FASTLY_API_TOKEN','FASTLY_LOG_ADAPTER_TOKEN'}
DEFAULTS=dict(line.split('=',1) for line in (ROOT/'.env.example').read_text().splitlines() if '=' in line and not line.startswith('#'))
def save(path,data):
 STATE.mkdir(mode=0o700,exist_ok=True);os.chmod(STATE,0o700)
 fd=os.open(path,os.O_WRONLY|os.O_CREAT|os.O_TRUNC,0o600)
 with os.fdopen(fd,'w') as f:json.dump(data,f,indent=2)
 os.chmod(path,0o600)
def read(path):return json.loads(path.read_text()) if path.exists() else {}
def config(args):
 c=DEFAULTS|read(STATE/'config.json')|read(STATE/'secrets.json')
 if args.config:
  p=Path(args.config)
  if p.stat().st_mode&0o077:raise ValueError('--config must have mode 0600')
  c.update(read(p))
 c.update({k:v for k,v in os.environ.items() if k in c or k.startswith('FASTLY_')})
 return c
def validate(c,cloud=False,require_keys=False):
 if not 1<=int(c['PORT'])<=65535:raise ValueError('PORT must be 1..65535')
 if c['SAMPLE_MODE'] not in ('local','cloud'):raise ValueError('SAMPLE_MODE must be local or cloud')
 if c['CORRIDORA_TARGET'] not in ('own','demo'):raise ValueError('CORRIDORA_TARGET must be own or demo')
 if c['CORRIDORA_TARGET']=='demo':c['DEPLOYMENT_ENVIRONMENT']='demo'
 if not re.fullmatch(r'([01]\d|2[0-3]):[0-5]\d',c['DEMO_DAILY_START_UTC']):raise ValueError('daily window must be HH:MM UTC')
 if not 1<=int(c['DEMO_WINDOW_SECONDS'])<=600:raise ValueError('window must be 1..600 seconds')
 for k in ['CORRIDORA_OTLP_ENDPOINT','CORRIDORA_RUM_SCRIPT_URL','PORTFOLIO_FIXTURE_URL','TRACKER_SIMULATOR_URL','FASTLY_LOG_ADAPTER_URL','PUBLIC_URL']:
  if not c.get(k):continue
  u=urllib.parse.urlsplit(c[k])
  if u.username or u.password or u.fragment or (u.scheme!='https' and not(u.scheme=='http' and u.hostname in ['localhost','127.0.0.1'])):raise ValueError(k+' requires HTTPS (localhost HTTP allowed), no URL credentials/fragments')
 if cloud and not c.get('PORTFOLIO_FIXTURE_URL'):raise ValueError('cloud mode needs controlled PORTFOLIO_FIXTURE_URL serving /api/fixture?slow=1; local mode has a standalone stub')
 if require_keys:
  for k in ['CORRIDORA_INGEST_KEY','CORRIDORA_RUM_KEY','DEMO_CONTROL_TOKEN']:
   if not c.get(k):raise ValueError(k+' required for cloud telemetry/scenario control')
  if not c.get('FASTLY_API_TOKEN'):raise ValueError('FASTLY_API_TOKEN required (scoped API token, never account password)')
 return c
def configure(c,a):
 keys=['SAMPLE_MODE','CORRIDORA_TARGET','CORRIDORA_OTLP_ENDPOINT','CORRIDORA_RUM_SCRIPT_URL','CORRIDORA_RUM_KEY','CORRIDORA_INGEST_KEY','OTEL_SERVICE_NAME','OTEL_SERVICE_NAMESPACE','DEPLOYMENT_ENVIRONMENT','PORT','DEMO_DAILY_START_UTC','DEMO_WINDOW_SECONDS','DEMO_CONTROL_TOKEN','ENABLE_SYNTHETICS','ENABLE_PROVIDER_FORWARDING','PORTFOLIO_FIXTURE_URL','DOWNSTREAM_AUTH_TOKEN','TRACKER_SIMULATOR_URL']
 if not a.non_interactive:
  for k in keys:
   default=c[k]
   prompt=f'{k} [{"saved" if default and k in SECRET else default}]: '
   v=getpass.getpass(prompt) if k in SECRET else input(prompt)
   if v:c[k]=v
  if c['SAMPLE_MODE']=='cloud':
   for k in ['FASTLY_API_TOKEN','FASTLY_CUSTOMER_ID','FASTLY_SERVICE_ID','FASTLY_RESOURCE_PREFIX','FASTLY_KV_STORE','FASTLY_CONFIG_STORE','FASTLY_SECRET_STORE','FASTLY_KV_ID','FASTLY_CONFIG_ID','FASTLY_SECRET_ID','FASTLY_LOG_ADAPTER_URL','FASTLY_LOG_ADAPTER_TOKEN','PUBLIC_URL','GITHUB_REPOSITORY']:
    v=getpass.getpass(f'{k} [saved/empty]: ') if k in SECRET else input(f'{k} [{c[k]}]: ')
    if v:c[k]=v
 c['DEMO_CONTROL_TOKEN']=c['DEMO_CONTROL_TOKEN'] or secrets.token_urlsafe(32)
 c['SERVICE_VERSION']=build_version()
 validate(c,cloud=c['SAMPLE_MODE']=='cloud',require_keys=c['SAMPLE_MODE']=='cloud')
 save(STATE/'config.json',{k:v for k,v in c.items() if k not in SECRET});save(STATE/'secrets.json',{k:v for k,v in c.items() if k in SECRET})
 save(STATE/'fastly-config.json',{k:v for k,v in c.items() if k not in SECRET})
 save(STATE/'fastly-secrets.json',{k:v for k,v in c.items() if k in ['CORRIDORA_INGEST_KEY','DEMO_CONTROL_TOKEN','DOWNSTREAM_AUTH_TOKEN']})
 save(STATE/'fastly-kv.json',{'catalog':json.dumps([{'id':'edge-cache','title':'Understand edge caching','minutes':8},{'id':'trace-context','title':'Follow W3C trace context','minutes':6},{'id':'async-rust','title':'Write bounded async Rust','minutes':10}])})
 print('Saved owner-only .sample/config.json and .sample/secrets.json. No backend keys enter browser assets.')
 return c
def build_version():
 r=subprocess.run(['git','rev-parse','--short=12','HEAD'],cwd=ROOT,capture_output=True,text=True)
 h=hashlib.sha256()
 for folder in ['src','web/src']:
  for p in sorted((ROOT/folder).rglob('*')):
   if p.is_file():h.update(p.relative_to(ROOT).as_posix().encode());h.update(p.read_bytes())
 for name in ['Cargo.lock','package-lock.json']:h.update((ROOT/name).read_bytes())
 return (r.stdout.strip()+'-' if r.returncode==0 else 'source-')+h.hexdigest()[:12]
def run_cmd(argv,c):
 env=os.environ|c|{'SAMPLE_BUILD_VERSION':build_version()}
 subprocess.run(argv,cwd=ROOT,env=env,check=True)
def build(c,cloud=False):
 run_cmd(['npm','ci','--ignore-scripts'],c);run_cmd(['npm','run','build'],c)
 if cloud:
  run_cmd(['cargo','build','--locked','--release','--features','compute','--bin','sample-tutorials','--target','wasm32-wasip1'],c)
  pkg=ROOT/'pkg';pkg.mkdir(exist_ok=True)
  # Fastly Compute package expects bin/main.wasm.
  with tarfile.open(pkg/'sample-tutorials.tar.gz','w:gz') as tar:tar.add(ROOT/'target/wasm32-wasip1/release/sample-tutorials.wasm',arcname='bin/main.wasm')
 else:run_cmd(['cargo','build','--locked','--bin','local'],c)
def api(c,method,path,data=None,form=False,binary=None,content_type=None):
 headers={'Fastly-Key':c['FASTLY_API_TOKEN'],'Accept':'application/json'}
 if binary is not None:body=binary;headers['Content-Type']=content_type
 elif data is None:body=None
 elif form:body=urllib.parse.urlencode(data).encode();headers['Content-Type']='application/x-www-form-urlencoded'
 else:body=json.dumps(data).encode();headers['Content-Type']='application/json'
 try:
  with urllib.request.urlopen(urllib.request.Request('https://api.fastly.com'+path,data=body,headers=headers,method=method),timeout=30) as r:
   b=r.read();return json.loads(b) if b else {}
 except urllib.error.HTTPError as e:raise RuntimeError(f'Fastly {method} {path}: HTTP {e.code}; response omitted to protect secrets') from None

def plan(c):
 if c['SAMPLE_MODE']=='local':return {'mode':'local','url':'http://127.0.0.1:'+c['PORT'],'resources':['Rust local process','Astro static build','Git-ignored local seeded stores'],'estimated_monthly_usd':'$0 local infrastructure'}
 return {'mode':c['SAMPLE_MODE'],'account':c.get('FASTLY_CUSTOMER_ID') or '(discover current customer before applying)','service':c.get('FASTLY_SERVICE_ID') or c['FASTLY_RESOURCE_PREFIX'],'resources':['Compute service/version/domain','KV tutorial-progress (catalog, progress, expiring scenario, heartbeat)','Config tutorial-config','Secret tutorial-secrets','otel-http and controlled portfolio-origin backends']+(['HTTPS log streaming endpoint (existing external adapter)'] if c['ENABLE_PROVIDER_FORWARDING']=='true' else []),'estimated_monthly_usd':'$0 within account allowances; fleet budget requires usage checks; logging adapter host external/shared','scheduled_journeys':'GitHub Actions daily 14:42 UTC plus 18:00 watchdog; delivery not guaranteed'}
def deploy(c,a):
 print(json.dumps(plan(c),indent=2))
 if a.dry_run:return
 if c['SAMPLE_MODE']=='local':build(c);return
 validate(c,True,True)
 if not a.apply:raise ValueError('Cloud deployment requires --apply after reviewing ./setup deploy --dry-run')
 customer=api(c,'GET','/current_customer');actual=customer.get('id','')
 if c['FASTLY_CUSTOMER_ID'] and c['FASTLY_CUSTOMER_ID']!=actual:raise ValueError('Fastly account does not match configured customer')
 c['FASTLY_CUSTOMER_ID']=actual;print('Applying to Fastly customer',actual)
 if c['ENABLE_SYNTHETICS']=='true':
  if not shutil.which('gh'):raise ValueError('GitHub CLI gh required to install daily/watchdog schedule secrets')
  run_cmd(['gh','auth','status'],c)
 build(c,True)
 inv=read(STATE/'resources.json');inv.setdefault('stores',{});inv.setdefault('owned_service',False)
 sid=inv.get('service_id') or c['FASTLY_SERVICE_ID']
 if not sid:
  obj=api(c,'POST','/service',{'name':c['FASTLY_RESOURCE_PREFIX'],'type':'wasm','comment':'Owned by Corridora sample-tutorials'},True);sid=obj['id'];inv['owned_service']=True;inv['service_id']=sid;save(STATE/'resources.json',inv)
 else:inv['service_id']=sid;save(STATE/'resources.json',inv)
 versions=api(c,'GET',f'/service/{sid}/version');active=next((x for x in versions if x.get('active')),None)
 version=str(api(c,'PUT',f'/service/{sid}/version/{active["number"]}/clone')['number']) if active else str(max(versions,key=lambda x:x['number'])['number'])
 prefix=f'/service/{sid}/version/{version}'
 # Existing version resources are cloned. PUT updates; POST creates only absent names.
 def ensure(kind,name,fields):
  existing=api(c,'GET',prefix+'/'+kind)
  if any(x.get('name')==name for x in existing) and not fields:return next(x for x in existing if x.get('name')==name)
  if any(x.get('name')==name for x in existing):return api(c,'PUT',prefix+'/'+kind+'/'+urllib.parse.quote(name,safe=''),fields,True)
  return api(c,'POST',prefix+'/'+kind,{'name':name}|fields,True)
 domain=urllib.parse.urlsplit(c['PUBLIC_URL']).hostname if c['PUBLIC_URL'] else c['FASTLY_RESOURCE_PREFIX']+'.edgecompute.app'
 ensure('domain',domain,{})
 c['PUBLIC_URL']='https://'+domain;c['FASTLY_SERVICE_ID']=sid;c['SERVICE_VERSION']=build_version()
 for kind,link,setting in [('kv','tutorial-progress','FASTLY_KV_STORE'),('config','tutorial-config','FASTLY_CONFIG_STORE'),('secret','tutorial-secrets','FASTLY_SECRET_STORE')]:
  store=inv['stores'].get(kind)
  if not store:
   selected=c.get('FASTLY_'+kind.upper()+'_ID')
   if selected:store={'id':selected,'owned':False}
   else:
    existing=api(c,'GET','/resources/stores/'+kind);items=existing if isinstance(existing,list) else existing.get('data',[])
    same=next((x for x in items if x.get('name')==c[setting]),None)
    if same:raise ValueError(f'Store {c[setting]} already exists; explicitly select its FASTLY_{kind.upper()}_ID to reuse it')
    store={'id':api(c,'POST','/resources/stores/'+kind,{'name':c[setting]})['id'],'owned':True}
   inv['stores'][kind]=store;save(STATE/'resources.json',inv)
  links=api(c,'GET',prefix+'/resource')
  if not any(x.get('name')==link for x in links):api(c,'POST',prefix+'/resource',{'name':link,'resource_id':store['id']},True)
 # Versionless writes are intentionally explicit and update only the selected stores.
 for k,v in c.items():
  if k in SECRET or k.startswith('FASTLY_API') or k.endswith('_ID') and k!='FASTLY_SERVICE_ID':continue
  api(c,'PUT',f'/resources/stores/config/{inv["stores"]["config"]["id"]}/item/'+urllib.parse.quote(k,safe=''),{'item_value':v},True)
 for k in ['CORRIDORA_INGEST_KEY','DEMO_CONTROL_TOKEN','DOWNSTREAM_AUTH_TOKEN']:
  if c.get(k):api(c,'PUT',f'/resources/stores/secret/{inv["stores"]["secret"]["id"]}/secrets',{'name':k,'secret':base64.b64encode(c[k].encode()).decode()})
 api(c,'PUT',f'/resources/stores/kv/{inv["stores"]["kv"]["id"]}/keys/catalog',binary=json.dumps([{'id':'edge-cache','title':'Understand edge caching','minutes':8},{'id':'trace-context','title':'Follow W3C trace context','minutes':6},{'id':'async-rust','title':'Write bounded async Rust','minutes':10}]).encode(),content_type='application/octet-stream')
 for name,url in [('otel-http',c['CORRIDORA_OTLP_ENDPOINT']),('portfolio-origin',c['PORTFOLIO_FIXTURE_URL'])]:
  u=urllib.parse.urlsplit(url);ensure('backend',name,{'address':u.hostname,'port':443,'use_ssl':1,'ssl_check_cert':1,'ssl_sni_hostname':u.hostname,'ssl_cert_hostname':u.hostname,'override_host':u.hostname,'connect_timeout':1000,'first_byte_timeout':3000,'between_bytes_timeout':1000})
 if c['ENABLE_PROVIDER_FORWARDING']=='true':
  if not c.get('FASTLY_LOG_ADAPTER_URL') or not c.get('FASTLY_LOG_ADAPTER_TOKEN'):raise ValueError('provider forwarding needs HTTPS adapter URL and dedicated token')
  # HTTPS endpoint authentication headers are written directly to the API over TLS, never argv.
  ensure('logging/https','corridora-edge',{'url':c['FASTLY_LOG_ADAPTER_URL'],'method':'POST','content_type':'application/x-ndjson','header_name':'Authorization','header_value':'Bearer '+c['FASTLY_LOG_ADAPTER_TOKEN'],'format':'%{message}V','format_version':2,'message_type':'blank'})
 boundary='corridora-'+secrets.token_hex(12);blob=(ROOT/'pkg/sample-tutorials.tar.gz').read_bytes();data=(f'--{boundary}\r\nContent-Disposition: form-data; name="package"; filename="sample-tutorials.tar.gz"\r\nContent-Type: application/gzip\r\n\r\n').encode()+blob+f'\r\n--{boundary}--\r\n'.encode()
 api(c,'PUT',prefix+'/package',binary=data,content_type='multipart/form-data; boundary='+boundary)
 api(c,'GET',prefix+'/validate');api(c,'PUT',prefix+'/activate');inv['version']=version;save(STATE/'resources.json',inv)
 save(STATE/'config.json',{k:v for k,v in c.items() if k not in SECRET});print('Deployed',c['PUBLIC_URL'])
 if c['ENABLE_SYNTHETICS']=='true':
  repo=c.get('GITHUB_REPOSITORY','corridora/sample-tutorials')
  for name,value in [('SAMPLE_URL',c['PUBLIC_URL']),('DEMO_CONTROL_TOKEN',c['DEMO_CONTROL_TOKEN'])]:subprocess.run(['gh','secret','set',name,'--repo',repo],input=value,text=True,cwd=ROOT,check=True)
  run_cmd(['gh','variable','set','ENABLE_SYNTHETICS','--body','true','--repo',repo],c)
  run_cmd(['gh','workflow','enable','daily.yml','--repo',repo],c)
  inv['workflow_enabled']=True;inv['github_repository']=repo;save(STATE/'resources.json',inv)
  print('Enabled committed daily.yml workflow and its dedicated secrets. GitHub schedules may be delayed or disabled after inactivity.')
 else:print('Synthetic workflow included but disabled. Set ENABLE_SYNTHETICS=true and rerun deploy to configure it; Fastly Compute has no built-in cron.')
def url(c):return c['PUBLIC_URL'] if c['SAMPLE_MODE']=='cloud' else 'http://127.0.0.1:'+c['PORT']
def http(c,path,data=None):
 headers={'Content-Type':'application/json'}
 if data is not None:headers['Authorization']='Bearer '+c['DEMO_CONTROL_TOKEN']
 with urllib.request.urlopen(urllib.request.Request(url(c)+path,data=json.dumps(data).encode() if data is not None else None,headers=headers),timeout=12) as r:return json.load(r)
def main():
 p=argparse.ArgumentParser(description='Configure, deploy, run, verify, scenario or destroy the standalone Astro/Rust Fastly sample.')
 p.add_argument('command',nargs='?',choices=['configure','deploy','run','verify','scenario','destroy'])
 p.add_argument('--non-interactive',action='store_true');p.add_argument('--config');p.add_argument('--dry-run','--plan',dest='dry_run',action='store_true');p.add_argument('--apply',action='store_true');p.add_argument('--phase',default='regression',choices=['healthy','regression','recover']);p.add_argument('--duration',type=int,default=120)
 a=p.parse_args();os.chdir(ROOT)
 try:
  c=config(a)
  if not a.command:
   if a.non_interactive:raise ValueError('choose an explicit subcommand for noninteractive use')
   c=configure(c,a);deploy(c,a)
   if c['SAMPLE_MODE']=='local':
    print('Starting sample and verifying the browser journey. Ctrl-C stops local server.')
    proc=subprocess.Popen([str(ROOT/'target/debug/local')],cwd=ROOT,env=os.environ|c)
    try:
     for _ in range(100):
      try:
       if http(c,'/healthz').get('ok'):break
      except urllib.error.URLError:time.sleep(0.1)
     else:raise RuntimeError('local app did not become healthy')
     run_cmd(['npx','playwright','install','chromium'],c)
     run_cmd(['npx','playwright','test'],c|{'SAMPLE_URL':url(c)})
     print('Browser workflow verified:',url(c),'— confirm stored browser/server parentage in Corridora UI. Ctrl-C stops app.')
     proc.wait()
    except KeyboardInterrupt:pass
    finally:proc.terminate();proc.wait(timeout=10)
   else:print(http(c,'/healthz'));run_cmd(['npx','playwright','test'],c|{'SAMPLE_URL':url(c)})
  elif a.command=='configure':configure(c,a)
  elif a.command=='deploy':deploy(c,a)
  elif a.command=='run':
   if c['SAMPLE_MODE']=='cloud':print(url(c),http(c,'/healthz'))
   else:
    if not (ROOT/'target/debug/local').exists():build(c)
    run_cmd([str(ROOT/'target/debug/local')],c)
  elif a.command=='verify':
   print('App',url(c));print('Backend trace check',http(c,'/api/trace-check'))
   run_cmd(['npx','playwright','install','chromium'],c)
   run_cmd(['npx','playwright','test'],c|{'SAMPLE_URL':url(c)})
   print('Browser journey passed. Confirm stored CLIENT→SERVER parentage, log, metric and provider data in Corridora UI; HTTP200 is delivery acknowledgment only.')
  elif a.command=='scenario':
   if not 1<=a.duration<=600:raise ValueError('duration 1..600 required')
   print(http(c,'/api/demo/scenario',{'phase':a.phase,'duration_seconds':a.duration}))
  elif a.command=='destroy':
   inv=read(STATE/'resources.json');print(json.dumps({'remove_owned_only':inv,'preserve':'all selected existing services/stores, dependencies and GitHub repository'},indent=2))
   if a.dry_run:return
   if c['SAMPLE_MODE']=='cloud':
    if not a.apply:raise ValueError('destroy requires --apply after dry run')
    if inv.get('workflow_enabled'):
     run_cmd(['gh','variable','set','ENABLE_SYNTHETICS','--body','false','--repo',inv['github_repository']],c);inv['workflow_enabled']=False;save(STATE/'resources.json',inv)
    if inv.get('owned_service'):
     sid=inv['service_id'];api(c,'PUT',f'/service/{sid}/version/{inv["version"]}/deactivate');api(c,'DELETE','/service/'+sid)
     inv['owned_service']=False;inv['service_removed']=True;save(STATE/'resources.json',inv)
    for kind,obj in list(inv.get('stores',{}).items()):
     if obj['owned']:
      if not inv.get('owned_service_deleted') and not inv.get('service_removed'):print('Preserving linked owned store',kind,'because the selected existing service remains; unlink it explicitly before removal');continue
      api(c,'DELETE','/resources/stores/'+kind+'/'+obj['id']);del inv['stores'][kind];save(STATE/'resources.json',inv)
   else:shutil.rmtree(STATE/'store',ignore_errors=True)
   print('Owned resources removed; protected config/secrets retained for reuse/rotation.')
 except (ValueError,RuntimeError,subprocess.CalledProcessError,urllib.error.URLError) as e:print('Setup failed:',e,file=sys.stderr);sys.exit(1)
