import unittest.mock
import importlib.util, pathlib, tempfile, unittest, os, json, sys
ROOT=pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'scripts'))
import setup_app as s
import provider_adapter as a
class SetupTests(unittest.TestCase):
 def test_defaults_plan_no_keys(self):
  c=s.DEFAULTS.copy();self.assertEqual(s.plan(c)['mode'],'local');self.assertNotIn('CORRIDORA_INGEST_KEY',json.dumps(s.plan(c)))
 def test_url_security(self):
  for value in ['http://public.example','https://secret:password@example.com/']:
   c=s.DEFAULTS|{'PORTFOLIO_FIXTURE_URL':value}
   with self.assertRaises(ValueError):s.validate(c)
 def test_duration(self):
  with self.assertRaises(ValueError):s.validate(s.DEFAULTS|{'DEMO_WINDOW_SECONDS':'601'})
 def test_cloud_controlled_origin_required(self):
  with self.assertRaises(ValueError):s.validate(s.DEFAULTS,cloud=True)
 def test_secret_permissions(self):
  old=s.STATE
  with tempfile.TemporaryDirectory() as tmp:
   s.STATE=pathlib.Path(tmp);p=s.STATE/'secret.json';s.save(p,{'key':'mock'});self.assertEqual(p.stat().st_mode&0o777,0o600)
  s.STATE=old
 def test_adapter_logs(self):
  v=a.convert_logs('{"event":"edge.request","traceId":"11111111111111111111111111111111","spanId":"2222222222222222","status":200}')
  r=v['resourceLogs'][0]['scopeLogs'][0]['logRecords'][0];self.assertEqual(r['traceId'],'1'*32);self.assertEqual(r['severityNumber'],9)
 def test_adapter_metrics(self):
  v=a.convert_stats({'data':[{'start_time':123,'requests':5,'compute_execution_time_ms':10}]});self.assertEqual(len(v['resourceMetrics'][0]['scopeMetrics'][0]['metrics']),2)
 def test_adapter_failed_batch_ledger(self):
  with tempfile.TemporaryDirectory() as tmp:
   with unittest.mock.patch.dict(os.environ,{'ADAPTER_STATE':tmp+'/adapter.sqlite'}):
    db=a.ledger();db.execute('INSERT INTO pending VALUES (?,?,?)',('fixture','{}',int(a.time.time())));db.commit();db.close();db=a.ledger();self.assertEqual(db.execute('SELECT COUNT(*) FROM pending').fetchone()[0],1);self.assertEqual(pathlib.Path(tmp+'/adapter.sqlite').stat().st_mode&0o777,0o600);db.close()
 def test_exporter_single_send_status_patch(self):
  text=(ROOT/'vendor/fastly-opentelemetry-exporter-0.2.0/src/span_exporter.rs').read_text();self.assertIn('.send(backend)',text);self.assertNotIn('.send_async(',text);self.assertIn('is_success()',text)
if __name__=='__main__':unittest.main()
