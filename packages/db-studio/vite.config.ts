import {defineConfig,loadEnv} from 'vite';
export default defineConfig(({mode})=>{
  const env=loadEnv(mode,'.','');
  const proxy={'/api':{target:env.DB_STUDIO_BACKEND || 'http://127.0.0.1:41902'}};
  return {base:'./',build:{cssCodeSplit:false,assetsInlineLimit:0},server:{port:41903,proxy},preview:{port:41903,proxy}};
});
