#!/usr/bin/env python3
"""Exercise the shipped private host against a synthetic site, without external traffic.
Usage: python3 private-host-smoke.py HOST_BINARY PINNED_CHROME EVIDENCE_DIR
The private proxy answers fixture.invalid locally and rejects every other host.
"""
import base64, http.server, json, os, pathlib, re, secrets, select, shutil, signal, subprocess, sys, tempfile, threading, time, urllib.parse, struct, zlib
binary,chrome,evidence=map(pathlib.Path,sys.argv[1:])
evidence.mkdir(parents=True,exist_ok=True)
def masked_pixels(png):
    offset=8;packed=b''
    while offset<len(png):
        length=struct.unpack('>I',png[offset:offset+4])[0];kind=png[offset+4:offset+8];data=png[offset+8:offset+8+length];offset+=length+12
        if kind==b'IHDR':width,height,depth,color,_,_,interlace=struct.unpack('>IIBBBBB',data);assert (depth,color,interlace)==(8,6,0)
        if kind==b'IDAT':packed+=data
    raw=zlib.decompress(packed);stride=width*4;prev=bytearray(stride);offset=0;count=0
    def paeth(a,b,c):
        p=a+b-c;pa,pb,pc=abs(p-a),abs(p-b),abs(p-c)
        return a if pa<=pb and pa<=pc else b if pb<=pc else c
    for _ in range(height):
        mode=raw[offset];offset+=1;row=bytearray(raw[offset:offset+stride]);offset+=stride
        for i in range(stride):
            a=row[i-4] if i>=4 else 0;b=prev[i];c=prev[i-4] if i>=4 else 0
            add=0 if mode==0 else a if mode==1 else b if mode==2 else (a+b)//2 if mode==3 else paeth(a,b,c)
            row[i]=(row[i]+add)&255
        count+=sum(row[i:i+4]==b'\x18\x18\x18\xff' for i in range(0,stride,4));prev=row
    return count
page='''<!doctype html><title>Private host fixture</title><h1>Fixture</h1>
<form id="fixture-form" action="http://fixture.invalid/submitted"><label for="query">Query</label><input id="query" name="query"><button type="submit">Submit</button></form><input form="fixture-form" aria-label="External form" name="associated">
<input aria-label="Readonly" readonly value="synthetic-readonly-secret" oninput="document.getElementById('result').textContent='readonly mutated'">
<label for="password">Password</label><input id="password" type="password" value="synthetic-password-secret">
<label for="otp">Code</label><input id="otp" autocomplete="one-time-code" value="synthetic-otp-secret">
<input aria-label="Card" autocomplete="cc-number" value="synthetic-card-secret"><div contenteditable="true">synthetic-editable-secret</div><input name="otp" aria-label="Alias"><input aria-label="KeySwap" id="keySwap" onkeydown="this.outerHTML='<input id=keySwap aria-label=KeySwap>'">
<button id="save" onclick="document.getElementById('result').textContent='saved'">Save</button>
<button onclick="document.getElementById('save').outerHTML='<button id=save>Save</button>'">Replace</button>
<div id="result"></div><span id="buy-label">Buy</span><button aria-label="Search" aria-labelledby="buy-label"><span aria-hidden="true">+</span></button><a href="http://fixture.invalid/two">Second page</a>'''
requests=[]
class Proxy(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        requests.append({'method':'GET','url':self.path})
        parsed=urllib.parse.urlparse(self.path)
        if parsed.hostname!='fixture.invalid':self.send_error(403);return
        if parsed.path=='/download':
            self.send_response(200);self.send_header('Content-Type','application/octet-stream');self.send_header('Content-Disposition','attachment; filename=clovy-should-not-save.txt');self.end_headers();self.wfile.write(b'synthetic denied download');return
        text=(page + ('<p>Persisted login</p>' if 'fixture_login=synthetic' in self.headers.get('Cookie','') else '')) if parsed.path=='/' else f'<title>Result</title><h1>{parsed.path}</h1><p>{parsed.query}</p>'
        self.send_response(200);self.send_header('Content-Type','text/html');self.send_header('Set-Cookie','fixture_login=synthetic; Max-Age=3600; Path=/; SameSite=Lax');self.end_headers();self.wfile.write(text.encode())
    def do_CONNECT(self):requests.append({'method':'CONNECT','url':self.path});self.send_error(403)
    def log_message(self,*args):pass
proxy=http.server.ThreadingHTTPServer(('127.0.0.1',0),Proxy);threading.Thread(target=proxy.serve_forever,daemon=True).start()
profile=pathlib.Path(tempfile.mkdtemp(prefix='clovy-private-fixture-'))
proc=None;xvfb=None;receipts=[];stderr_file=None
try:
    env={'HOME':str(profile),'PATH':'/usr/bin:/bin'}
    if shutil.which('Xvfb'):
        display=f':{200+os.getpid()%2000}'
        xvfb=subprocess.Popen(['Xvfb',display,'-screen','0','1280x900x24','-nolisten','tcp'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        env['DISPLAY']=display;time.sleep(.25)
        if xvfb.poll() is not None:raise RuntimeError('Owned Xvfb failed; refusing another display')
    stderr_file=(evidence/'host-stderr.txt').open('w+')
    proc=subprocess.Popen([str(binary.resolve())],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=stderr_file,text=True,bufsize=1,env=env,start_new_session=True)
    def wire(v):
        proc.stdin.write(json.dumps(v)+'\n');proc.stdin.flush()
        if not select.select([proc.stdout],[],[],25)[0]:raise TimeoutError('Private host response deadline')
        line=proc.stdout.readline()
        if not line:
            stderr_file.flush();stderr_file.seek(0);raise RuntimeError('Host exited: '+stderr_file.read())
        return json.loads(line)
    token=secrets.token_hex(32)
    assert wire({'protocol':1,'token':token})=={'protocol':1,'token':token}
    def call(action,ok=True,**args):
        receipt=wire({'id':len(receipts)+1,'action':action,**args});receipts.append(receipt)
        (evidence/'receipts.json').write_text(json.dumps(receipts,indent=2))
        if ok and 'error' in receipt:
            if action=='navigate':
                time.sleep(2)
                diagnostic=wire({'id':999,'action':'snapshot','tab_id':args['tab_id']})
                (evidence/'post-failure-observation.json').write_text(json.dumps(diagnostic,indent=2))
            raise AssertionError((action,receipt))
        return receipt.get('data',{}) if ok else receipt
    invalid=call('eval',ok=False,script='document.body.textContent')
    assert invalid['error']['code']=='invalid_request' and invalid['delivery']=='not_attempted'
    start=call('start',chromePath=str(chrome.resolve()),profilePath=str(profile/'browser'),proxyPort=proxy.server_port,persistent=True,headed=False)
    tab=start['tabId'];assert isinstance(tab,int)
    assert call('navigate',tab_id=tab,url='http://fixture.invalid/')['url']=='http://fixture.invalid/'
    snap=call('snapshot',tab_id=tab)['snapshot']
    assert all(secret not in snap for secret in ['synthetic-password-secret','synthetic-otp-secret','synthetic-card-secret','synthetic-editable-secret','synthetic-readonly-secret'])
    (evidence/'snapshot.txt').write_text(snap)
    def ref(name,snapshot=None):
        if snapshot is None:snapshot=snap
        match=re.search(r'"'+re.escape(name)+r'"[^\n]*ref=([a-f0-9]{32})',snapshot)
        assert match,(name,snapshot)
        return match.group(1)
    def act(reference,operation,**params):
        expected=call('inspect_reference',tab_id=tab,ref=reference)
        assert expected['backendNodeId'] and expected['documentId'] and expected['origin']=='http://fixture.invalid'
        return call('guarded_action',tab_id=tab,ref=reference,operation=operation,expected=expected,**params)
    alias=ref('Alias');expected_alias=call('inspect_reference',tab_id=tab,ref=alias)
    assert expected_alias['element']['name']=='otp' and expected_alias['element']['label']=='Alias'
    denied=call('guarded_action',ok=False,tab_id=tab,ref=alias,operation='fill',text='forbidden synthetic',expected=expected_alias)
    assert denied['delivery']=='not_attempted' and denied['error']['code']=='sensitive_field'
    buy=re.search(r'- button "Buy"[^\n]*ref=([a-f0-9]{32})',snap);assert buy,snap
    labelled=call('inspect_reference',tab_id=tab,ref=buy.group(1))
    assert labelled['element']['tag']=='button' and labelled['element']['label']=='Buy' and labelled['element']['name']=='',labelled
    external=call('inspect_reference',tab_id=tab,ref=ref('External form'))
    assert external['element']['inForm'] is True and external['element']['name']=='associated',external
    readonly=ref('Readonly');readonly_expected=call('inspect_reference',tab_id=tab,ref=readonly)
    denied_edit=call('guarded_action',ok=False,tab_id=tab,ref=readonly,operation='press',key='Backspace',expected=readonly_expected)
    assert denied_edit['delivery']=='not_attempted' and denied_edit['error']['code']=='unsupported_operation',denied_edit
    assert 'readonly mutated' not in call('snapshot',tab_id=tab)['snapshot']
    snap=call('snapshot',tab_id=tab)['snapshot']
    query=ref('Query');act(query,'fill',text='synthetic query')
    snap2=call('snapshot',tab_id=tab)['snapshot'];assert 'synthetic query' not in snap2
    shot=call('screenshot',tab_id=tab);assert shot['url']=='http://fixture.invalid/' and shot['tabId']==tab;png=base64.b64decode(shot['base64']);assert png.startswith(b'\x89PNG\r\n\x1a\n');assert masked_pixels(png)>1000;(evidence/'screenshot.png').write_bytes(png)
    save=ref('Save',snap2);expected=call('inspect_reference',tab_id=tab,ref=save)
    act(ref('Replace',snap2),'click')
    stale=call('guarded_action',ok=False,tab_id=tab,ref=save,operation='click',expected=expected)
    assert stale['delivery']=='not_attempted' and stale['error']['code']=='stale_reference',stale
    snap3=call('snapshot',tab_id=tab)['snapshot']
    swap=ref('KeySwap',snap3);expected_swap=call('inspect_reference',tab_id=tab,ref=swap)
    uncertain=call('guarded_action',ok=False,tab_id=tab,ref=swap,operation='press',key='Enter',expected=expected_swap)
    assert uncertain['delivery']=='unknown' and uncertain['error']['code']=='effect_unknown',uncertain
    assert call('inspect_reference',ok=False,tab_id=tab,ref=ref('Query',snap3))['error']['code']=='stale_reference'
    snap3=call('snapshot',tab_id=tab)['snapshot'];act(ref('Query',snap3),'press',key='Enter')
    submitted=call('snapshot',tab_id=tab)['url'];assert 'submitted' in submitted and urllib.parse.parse_qs(urllib.parse.urlsplit(submitted).query)['query']==['synthetic query']
    call('back',tab_id=tab);assert call('snapshot',tab_id=tab)['url']=='http://fixture.invalid/'
    tab2=call('tab_open',url='http://fixture.invalid/two')['tabId'];assert tab2!=tab
    assert len(call('tab_list')['tabs'])==2
    call('tab_switch',tab_id=tab);call('tab_close',tab_id=tab2)
    closed=call('snapshot',ok=False,tab_id=tab2);assert closed['error']['code']=='tab_gone' and closed['delivery']=='not_attempted'
    old=call('snapshot',tab_id=tab)['snapshot'];oldref=ref('Query',old)
    call('suspend');assert call('snapshot',ok=False,tab_id=tab)['error']['code']=='human_takeover'
    call('resume');assert call('inspect_reference',ok=False,tab_id=tab,ref=oldref)['error']['code']=='stale_reference'
    denied_download=call('navigate',ok=False,tab_id=tab,url='http://fixture.invalid/download')
    assert denied_download.get('error') and denied_download['delivery']=='unknown',denied_download
    call('snapshot',tab_id=tab)
    assert not list(profile.rglob('clovy-should-not-save*')) and not list(profile.rglob('*.crdownload'))
    if xvfb:
        takeover=call('takeover');assert takeover['mode']=='visible' and takeover['suspended'] and takeover['tabId']>tab2
        assert call('snapshot',ok=False,tab_id=takeover['tabId'])['error']['code']=='human_takeover'
        call('resume');call('navigate',tab_id=takeover['tabId'],url='http://fixture.invalid/')
        assert 'Persisted login' in call('snapshot',tab_id=takeover['tabId'])['snapshot']
        if shutil.which('xdotool'):
            # Simulate the human only on this run's private X server. No product
            # CDP/eval API exists for the model to navigate a local file.
            sentinel=profile/'human-file.html';sentinel.write_text('<title>Clovy file sentinel</title><h1>Synthetic file must remain private</h1>')
            def xdotool(*arguments):
                return subprocess.check_output(['xdotool',*arguments],env=env,text=True,timeout=5).strip()
            window=xdotool('search','--onlyvisible','--class','Google-chrome').splitlines()[0]
            def human_url(url,title):
                xdotool('windowfocus','--sync',window)
                xdotool('key','--clearmodifiers','ctrl+l')
                xdotool('type','--clearmodifiers','--delay','0','--',url)
                xdotool('key','Return')
                deadline=time.monotonic()+5
                while time.monotonic()<deadline:
                    if title in xdotool('getwindowname',window):return
                    time.sleep(.05)
                raise TimeoutError('Human fixture navigation did not become visible')
            call('suspend');human_url(sentinel.as_uri(),'Clovy file sentinel');call('resume')
            file_capture=call('screenshot',ok=False,tab_id=takeover['tabId'])
            assert file_capture['delivery']=='not_attempted' and file_capture['error']['code']=='invalid_url' and 'data' not in file_capture,file_capture
            call('suspend');human_url('http://fixture.invalid/','Private host fixture');call('resume')
            assert call('screenshot',tab_id=takeover['tabId'])['url']=='http://fixture.invalid/'
    # Neither the host nor Chrome has a command socket / TCP CDP argument.
    parents={}
    for entry in pathlib.Path('/proc').iterdir():
        if not entry.name.isdigit():continue
        try:
            fields=(entry/'stat').read_text().rsplit(')',1)[1].split();parents[int(entry.name)]=int(fields[1])
        except (FileNotFoundError,ProcessLookupError,PermissionError):pass
    descendants={proc.pid}
    while True:
        found={pid for pid,ppid in parents.items() if ppid in descendants};before=len(descendants);descendants.update(found)
        if before==len(descendants):break
    for pid in descendants:
        try:
            assert os.getpgid(pid)==proc.pid,('escaped process group',pid)
            args=pathlib.Path('/proc',str(pid),'cmdline').read_bytes().replace(b'\0',b' ').decode(errors='replace')
            assert '--remote-debugging-port' not in args,args
            if '--user-data-dir' in args:assert '--password-store=basic' not in args and '--use-mock-keychain' not in args
        except (FileNotFoundError,ProcessLookupError):pass
    assert len(descendants)>1
    call('shutdown');proc.wait(timeout=5);assert proc.returncode==0
    (evidence/'receipts.json').write_text(json.dumps(receipts,indent=2))
    print(json.dumps({'passed':True,'receipts':len(receipts),'takeover':bool(xvfb),'owned_processes':len(descendants)}))
finally:
    if proc:
        try:os.killpg(proc.pid,signal.SIGKILL)
        except ProcessLookupError:pass
        proc.wait()
        stderr_file.flush();stderr_file.close()
    if xvfb:xvfb.terminate();xvfb.wait()
    proxy.shutdown();(evidence/'proxy-requests.json').write_text(json.dumps(requests,indent=2));shutil.rmtree(profile)
