import json
import zipfile
from pathlib import Path

root=Path(__file__).parent/'linux-cam/native-input-linux-cam-geometry-1'
def diff(a,b,path=''):
    if type(a) != type(b):
        return [(path,a,b)]
    if isinstance(a,dict):
        return [d for k in sorted(a.keys()|b.keys()) for d in diff(a.get(k),b.get(k),path+'/'+k)]
    if isinstance(a,list):
        if len(a)!=len(b):return [(path+'/length',len(a),len(b))]
        return [d for i,(x,y) in enumerate(zip(a,b)) for d in diff(x,y,path+'/'+str(i))]
    return [] if a==b else [(path,a,b)]

archive=root/'evidence/geometry-drill-holes-picked.nbcad'
with zipfile.ZipFile(archive) as z:saved=json.loads(z.read('model.json'))
print('saved CAM',json.dumps(saved.get('cam'),ensure_ascii=False)[:2000])
rows=[]
for path in (root/'sessions').glob('*/model.json'):
    m=json.loads(path.read_text(encoding='utf-8-sig'))
    if isinstance(m,str): m=json.loads(m)
    if not isinstance(m,dict): continue
    d=diff(saved,m)
    rows.append((len(d),path,d))
for n,p,d in sorted(rows,key=lambda row:row[0])[:8]:
    print(str(p),n)
    for path,a,b in d[:15]:print(path,repr(a)[:200],repr(b)[:200])

print('LATEST SNAPSHOTS')
order=[]
for path in (root/'sessions').glob('*/heartbeat.json'):
    h=json.loads(path.read_text(encoding='utf-8-sig'))
    order.append((h.get('updated_ms',0),path.parent))
for updated, directory in sorted(order,reverse=True)[:10]:
    m=json.loads((directory/'model.json').read_text(encoding='utf-8-sig'))
    print(updated,directory.name)
    for path,a,b in diff(saved,m)[:10]:print(path,repr(a)[:150],repr(b)[:150])
