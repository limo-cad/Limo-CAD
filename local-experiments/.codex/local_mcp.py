import json, os, pathlib, queue, subprocess, sys, threading

root = pathlib.Path(__file__).resolve().parents[1]
evidence = pathlib.Path((root / '.codex-evidence-path.log').read_text().strip())
request = json.loads(pathlib.Path(sys.argv[1]).read_text(encoding='utf-8-sig') if len(sys.argv)>1 else sys.stdin.read())
env = dict(os.environ)
env['NBCAD_SESSION_DIR'] = request.get('registry', str(evidence / 'sessions'))
env['PATH'] = str(root / 'vcpkg_installed/x64-windows/bin') + os.pathsep + env['PATH']
exe = pathlib.Path(request.get('server', str(evidence / 'host-raster-validated/nbcad.exe')))
p = subprocess.Popen([str(exe), '--headless'], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                     stderr=subprocess.PIPE, text=True, encoding='utf-8', env=env,
                     creationflags=subprocess.CREATE_NO_WINDOW)
q = queue.Queue()
def read():
    for line in p.stdout:
        q.put(json.loads(line))
threading.Thread(target=read, daemon=True).start()
serial = 0
def rpc(method, params):
    global serial
    serial += 1
    p.stdin.write(json.dumps({'jsonrpc':'2.0','id':serial,'method':method,'params':params})+'\n')
    p.stdin.flush()
    while True:
        reply = q.get(timeout=60)
        if reply.get('id') != serial:
            continue
        if 'error' in reply:
            raise RuntimeError(reply['error'])
        return reply['result']
def call(name, args):
    result = rpc('tools/call', {'name':name,'arguments':args})
    if result.get('isError'):
        raise RuntimeError(result)
    if 'structuredContent' in result:
        return result['structuredContent']
    text = next(c['text'] for c in result['content'] if c['type']=='text')
    try:
        return json.loads(text)
    except ValueError:
        return text
try:
    rpc('initialize', {'protocolVersion':'2025-06-18','capabilities':{},'clientInfo':{'name':'native-validation','version':'1'}})
    p.stdin.write('{"jsonrpc":"2.0","method":"notifications/initialized"}\n')
    p.stdin.flush()
    if request.get('session'):
        call('cad_attach', {'session_id':request['session']})
    if request.get('control'):
        inspected = call('cad_interface', {'action': 'inspect'})
        matches = [c for surface in inspected['ui']['surfaces'] for c in surface['controls']
                   if c['label'] == request['control'] and not c.get('disabled')]
        if len(matches) != 1:
            raise RuntimeError(f"Expected one enabled {request['control']}: {matches}")
        result = call('cad_interface', {'action': 'click', 'target': matches[0]['id']})
    else:
        result = call(request.get('tool','cad_interface'),request.get('args',{}))
    if request.get('output'):
        pathlib.Path(request['output']).write_text(json.dumps(result,indent=2),encoding='utf-8')
    print(json.dumps(result,ensure_ascii=True))
finally:
    p.stdin.close()
    try:
        p.wait(timeout=3)
    except subprocess.TimeoutExpired:
        p.kill()
        p.wait()
