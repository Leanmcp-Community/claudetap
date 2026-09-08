"""Integration test against the real Rust CLI and an isolated local server."""
import hashlib
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import time
import unittest
import httpx

class CliTest(unittest.TestCase):
    def test_upload_resume_and_filter(self):
        binary=Path(__file__).resolve().parents[1]/'target/debug/claudetap'
        if not binary.exists(): self.skipTest('Run cargo build first')
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp)
            key='synthetic-test-upload-key-000000000000'
            (root/'keys.json').write_text(json.dumps({hashlib.sha256(key.encode()).hexdigest():'test'}))
            with socket.socket() as s:
                s.bind(('127.0.0.1',0)); port=s.getsockname()[1]
            env={**os.environ,'CLAUDETAP_HOME':str(root/'client'),'CLAUDETAP_UPLOAD_KEY':key,'CLAUDETAP_DATA':str(root/'data'),'CLAUDETAP_KEYS_FILE':str(root/'keys.json')}
            import sys
            proc=subprocess.Popen([sys.executable,'-m','uvicorn','app:app','--host','127.0.0.1','--port',str(port)],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
            endpoint=f'http://127.0.0.1:{port}'
            try:
                for _ in range(100):
                    try:
                        if httpx.get(endpoint+'/health').status_code==200: break
                    except httpx.ConnectError: pass
                    time.sleep(.05)
                else: self.fail('Server did not start')
                def cli(*args):
                    return subprocess.run([str(binary),'cloud',*args],env=env,check=True,capture_output=True,text=True)
                cli('configure','--endpoint',endpoint,'--backfill')
                session='01ARZ3NDEKTSV4RRFFQ69G5FAV'
                p=root/'client/sessions'/session
                (p/'stream').mkdir(parents=True)
                (p/'meta.json').write_text('{"hostname":"test"}')
                (p/'traffic.jsonl').write_text('{"headers":[["Authorization","synthetic-secret"]]}\n')
                stream=p/'stream/request.sse.jsonl'
                stream.write_text('{"data":"first"}\n{"data":')
                cli('sync','--once')
                cli('sync','--once')
                headers={'Authorization':'Bearer '+key}
                sessions=httpx.get(endpoint+'/v1/sessions',headers=headers).json()
                self.assertEqual(len(sessions),1)
                device=sessions[0]['device']
                url=f'{endpoint}/v1/chunks/{device}/{session}'
                r=httpx.get(url,params={'path':'traffic.jsonl'},headers=headers)
                self.assertNotIn('synthetic-secret',r.text)
                r=httpx.get(url,params={'path':'stream/request.sse.jsonl'},headers=headers)
                self.assertEqual(r.text,'{"data":"first"}\n')
                offset=int(r.headers['X-Source-Length'])
                with stream.open('a') as f: f.write('"second"}\n')
                cli('sync','--once')
                r=httpx.get(url,params={'path':'stream/request.sse.jsonl','offset':offset},headers=headers)
                self.assertEqual(r.text,'{"data":"second"}\n')
                cli('disable')
                self.assertIn('false',cli('status').stdout)
            finally:
                proc.terminate(); proc.wait(timeout=10)

if __name__=='__main__': unittest.main()
