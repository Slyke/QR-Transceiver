#!/usr/bin/env python3
"""End-to-end IPC, REST auth and official MCP SDK smoke test, using only local processes."""
import json, os, pathlib, subprocess, tempfile, time, urllib.request, urllib.error, socket, select
ROOT=pathlib.Path(__file__).resolve().parent.parent
BINARY=ROOT/'target/debug/qrtransceiver'
def request(proc,method,params,identifier):
    proc.stdin.write(json.dumps({'jsonrpc':'2.0','id':identifier,'method':method,'params':params})+'\n');proc.stdin.flush()
    end=time.time()+15
    while time.time()<end:
        if not select.select([proc.stdout],[],[],.2)[0]: continue
        line=proc.stdout.readline()
        if not line: raise RuntimeError('MCP process exited')
        result=json.loads(line)
        if result.get('id')==identifier:return result
    raise TimeoutError(method)
with tempfile.TemporaryDirectory(prefix='qrtx-smoke-') as name:
    state=pathlib.Path(name)/'state'
    with socket.socket() as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
    service=subprocess.Popen([str(BINARY),'--state',str(state),'serve','--http',str(port),'--allow-root',str(ROOT/'tests/fixtures')],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
    mcp=None
    try:
        for _ in range(100):
            if (state/'engine.sock').exists():break
            if service.poll() is not None:raise RuntimeError('service exited')
            time.sleep(.1)
        status=json.loads(subprocess.check_output([str(BINARY),'--state',str(state),'status']))
        assert status['version']=='0.1.0'
        cfg=json.loads((state/'config.json').read_text())
        try:
            urllib.request.urlopen(f'http://127.0.0.1:{port}/v1/status')
            raise AssertionError('unauthenticated request succeeded')
        except urllib.error.HTTPError as error:assert error.code in [400,401,403]
        token=cfg['tokens'][0]['token']
        req=urllib.request.Request(f'http://127.0.0.1:{port}/v1/status',headers={'Authorization':'Bearer '+token})
        assert json.load(urllib.request.urlopen(req))['ok']
        mcp=subprocess.Popen([str(BINARY),'--state',str(state),'mcp'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,text=True,bufsize=1)
        initialized=request(mcp,'initialize',{'protocolVersion':'2025-06-18','capabilities':{},'clientInfo':{'name':'qrtx-smoke','version':'1'}},1)
        assert 'result' in initialized,initialized
        mcp.stdin.write(json.dumps({'jsonrpc':'2.0','method':'notifications/initialized'})+'\n');mcp.stdin.flush()
        tools=request(mcp,'tools/list',{},2)['result']['tools']
        assert len(tools)==13
        names={t['name'] for t in tools}
        assert {'qr_send_file','qr_get_status','qr_cancel_transfer'}<=names
        result=request(mcp,'tools/call',{'name':'qr_get_status','arguments':{}},3)['result']
        assert not result.get('isError',False),result
        preview=request(mcp,'tools/call',{'name':'qr_send_file','arguments':{'path':str(ROOT/'tests/fixtures/hello.txt')}},4)['result']
        assert json.loads(preview['content'][0]['text'])['applied'] is False
        denied=request(mcp,'tools/call',{'name':'qr_send_file','arguments':{'path':str(ROOT/'tests/fixtures/hello.txt'),'apply':True}},5)['result']
        assert denied.get('isError'),denied
        print(json.dumps({'ok':True,'checks':['local IPC','authenticated REST','MCP initialize','13 tool schemas','MCP status','dry run','read-only MCP write rejection']}))
    finally:
        if mcp:
            mcp.terminate()
            try:mcp.wait(timeout=5)
            except subprocess.TimeoutExpired:mcp.kill();mcp.wait()
        service.terminate()
        try:service.wait(timeout=5)
        except subprocess.TimeoutExpired:service.kill();service.wait()
