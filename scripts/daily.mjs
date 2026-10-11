#!/usr/bin/env node
import {spawnSync} from 'node:child_process';
const base=process.env.SAMPLE_URL;const token=process.env.DEMO_CONTROL_TOKEN;
if(!base||!token)throw new Error('SAMPLE_URL and DEMO_CONTROL_TOKEN are required');
if(process.argv.includes('--watchdog')){const r=await fetch(base+'/api/demo/status');const s=await r.json();const today=Math.floor(Date.now()/86400000);if(s.heartbeat?.result==='passed'&&Math.floor(s.heartbeat.at/86400)===today){console.log('Today’s browser heartbeat exists');process.exit(0);}}
// Full journey always opens an expiring override, exercises affected origin, verifies control/recovery and posts heartbeat.
const r=spawnSync('npx',['playwright','test'],{stdio:'inherit',env:process.env,timeout:180000});if(r.status!==0)process.exit(r.status||1);
