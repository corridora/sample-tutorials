import { defineConfig } from '@playwright/test';
export default defineConfig({testDir:'tests',testMatch:'journey.spec.mjs',workers:1,timeout:120000,use:{headless:true,baseURL:process.env.SAMPLE_URL||'http://127.0.0.1:4106'},reporter:'list'});
