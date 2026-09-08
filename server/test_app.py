import hashlib
import json
import tempfile
import unittest
from pathlib import Path
from fastapi.testclient import TestClient
import app

class UploadTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        app.DATA = Path(self.tmp.name)
        app.KEY_FILE = str(app.DATA / 'keys.json')
        Path(app.KEY_FILE).write_text(json.dumps({hashlib.sha256(k.encode()).hexdigest(): o for k,o in [('a'*32,'one'),('b'*32,'two')]}))
        self.client = TestClient(app.app)
        self.device = self.session = '01ARZ3NDEKTSV4RRFFQ69G5FAV'
    def tearDown(self):
        self.tmp.cleanup()
    def headers(self, data=b'hello', offset=0, key='a'*32, path='traffic.jsonl'):
        return {'Authorization': 'Bearer '+key, 'X-Device': self.device, 'X-Session': self.session, 'X-Path': path, 'X-Offset': str(offset), 'X-Source-Length': str(len(data)), 'X-SHA256': hashlib.sha256(data).hexdigest()}
    def test_retry_and_order(self):
        for _ in range(2):
            self.assertEqual(self.client.post('/v1/chunks', headers=self.headers(), content=b'hello').status_code,200)
        with app.db() as conn:
            self.assertEqual(conn.execute('SELECT COUNT(*) FROM chunks').fetchone()[0],1)
        self.assertEqual(self.client.post('/v1/chunks', headers=self.headers(offset=2), content=b'hello').status_code,409)
        self.assertEqual(self.client.post('/v1/chunks', headers=self.headers(offset=5), content=b'hello').status_code,200)
    def test_isolation_validation_revocation(self):
        self.client.post('/v1/chunks', headers=self.headers(), content=b'hello')
        self.assertEqual(self.client.get('/v1/sessions', headers={'Authorization':'Bearer '+'b'*32}).json(),[])
        self.assertEqual(self.client.get('/v1/sessions').status_code,401)
        self.assertEqual(self.client.post('/v1/chunks', headers=self.headers(path='../ca/root.key'), content=b'hello').status_code,400)
        self.assertEqual(self.client.post('/v1/chunks', headers=self.headers(), content=b'wrong').status_code,400)
        Path(app.KEY_FILE).write_text('{}')
        self.assertEqual(self.client.get('/v1/sessions', headers=self.headers()).status_code,401)
    def test_metadata_versions_and_download(self):
        for data in [b'{"v":1}',b'{"v":2}']:
            self.assertEqual(self.client.post('/v1/chunks',headers=self.headers(data,path='meta.json'),content=data).status_code,200)
        r=self.client.get(f'/v1/chunks/{self.device}/{self.session}?path=meta.json',headers=self.headers())
        self.assertEqual(r.content,b'{"v":2}')

if __name__ == '__main__': unittest.main()
