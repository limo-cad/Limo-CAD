import argparse, json, os, pathlib, subprocess, time

p = argparse.ArgumentParser()
p.add_argument('fixture')
p.add_argument('root')
p.add_argument('--server', required=True)
p.add_argument('--driver')
p.add_argument('--keep', action='store_true')
p.add_argument('--desktop-input', action='store_true')
p.add_argument('--mcp-only', action='store_true')
a = p.parse_args()
root = pathlib.Path(a.root).resolve()
root.mkdir(parents=True, exist_ok=False)
repo = pathlib.Path(__file__).resolve().parents[1]
server = pathlib.Path(a.server).resolve()
registry = root / 'sessions'
registry.mkdir()
env = dict(os.environ, NBCAD_SESSION_DIR=str(registry), NBCAD_CONFIG_DIR=str(root/'config'))
env['PATH'] = str(repo / 'vcpkg_installed/x64-windows/bin') + os.pathsep + env['PATH']
with (root/'host.stdout.log').open('w', encoding='utf-8') as output, (root/'host.stderr.log').open('w', encoding='utf-8') as error:
    host = subprocess.Popen([str(server)], cwd=root, env=env, stdout=output, stderr=error, creationflags=subprocess.CREATE_NO_WINDOW)
    (root/'host.json').write_text(json.dumps({'pid':host.pid,'exe':str(server)}), encoding='utf-8')
    try:
        deadline = time.monotonic()+60
        session = None
        while time.monotonic() < deadline:
            if host.poll() is not None:
                raise RuntimeError('Owned native host exited before publication')
            for entry in (registry/'_ui/processes').glob('*.json'):
                try:
                    info=json.loads(entry.read_text())
                    if info['pid']==host.pid:
                        session=info['windows'][0]['active_session_id']
                except (OSError, ValueError, KeyError, IndexError):
                    pass
            if session: break
            time.sleep(.1)
        if not session: raise RuntimeError('Owned native session did not publish')
        # Publication precedes the first retained UI layout.
        time.sleep(2)
        (root/'session.txt').write_text(session)
        driver = pathlib.Path(a.driver).resolve() if a.driver else repo/'target/debug/xtask.exe'
        result = subprocess.run([str(driver),'test-mcp',a.fixture,'--server',str(server),'--session',session,'--out',str(root/'evidence')] + (['--desktop-input'] if a.desktop_input else []) + (['--mcp-only'] if a.mcp_only else []), cwd=repo, env=env)
        raise SystemExit(result.returncode)
    finally:
        if not a.keep and host.poll() is None:
            host.terminate()
            host.wait(timeout=10)
