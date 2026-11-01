import urllib.request,urllib.error,json,pathlib,time,hashlib,sys
base=sys.argv[1] if len(sys.argv)>1 else 'http://localhost:9292'
out=pathlib.Path(sys.argv[2] if len(sys.argv)>2 else 'docs/reference/ruby-broker/2.121.2/minimal-ui');out.mkdir(parents=True,exist_ok=True)
class NoRedirect(urllib.request.HTTPRedirectHandler):
 def redirect_request(self,*a,**kw):return None
opener=urllib.request.build_opener(NoRedirect)
def call(path,accept='application/hal+json',method='GET',body=None):
 url=path if path.startswith('http') else base+path
 h={'Accept':accept} if accept is not None else {}
 if body is not None:h['Content-Type']='application/json'
 req=urllib.request.Request(url,headers=h,method=method,data=None if body is None else json.dumps(body).encode())
 try:r=opener.open(req,timeout=15)
 except urllib.error.HTTPError as e:r=e
 return r.status,dict(r.headers),r.read()
for _ in range(100):
 try:
  if call('/')[0]==200:break
 except OSError:pass
 time.sleep(.2)
else:raise RuntimeError('broker not ready')
seed=[]
def write(path,method,body):
 status,headers,data=call(path,method=method,body=body);assert status in [200,201],(path,status,data)
 seed.append({'method':method,'path':path.replace(base,''),'body':body,'status':status})
 return json.loads(data) if data else None
for version,status in [('1',200),('2',201),('3',202)]:
 pact={'consumer':{'name':'UiCheckout'},'provider':{'name':'UiPayments'},'interactions':[{'description':'an order lookup','providerState':'order 42 exists','request':{'method':'GET','path':'/orders/42'},'response':{'status':status,'body':{'id':42,'state':'ready'}}}],'metadata':{'pactSpecification':{'version':'2.0.0'}}}
 d=write(f'/pacts/provider/UiPayments/consumer/UiCheckout/version/{version}','PUT',pact)
 write(f'/pacticipants/UiCheckout/branches/main/versions/{version}','PUT',{})
 if version!='3':write(d['_links']['pb:publish-verification-results']['href'],'POST',{'providerApplicationVersion':'2','success':version=='1','providerVersionBranch':'main'})
write('/pacts/provider/UiInventory/consumer/UiOrders/version/1','PUT',{'consumer':{'name':'UiOrders'},'provider':{'name':'UiInventory'},'interactions':[]})
env=write('/environments','POST',{'name':'ui-test','production':False})
write(f'/pacticipants/UiPayments/versions/2/deployed-versions/environment/{env["uuid"]}','POST',{})
(out/'seed.json').write_text(json.dumps(seed,indent=2)+'\n')
paths={
 'index':'/', 'integrations':'/integrations',
 'pact':'/pacts/provider/UiPayments/consumer/UiCheckout/version/1',
 'latest-pact':'/pacts/provider/UiPayments/consumer/UiCheckout/latest',
 'matrix-pair':'/matrix/provider/UiPayments/consumer/UiCheckout',
 'matrix-query':'/matrix?q[][pacticipant]=UiCheckout&q[][pacticipant]=UiPayments&latestby=cvpv',
 'deploy-allowed':'/can-i-deploy?pacticipant=UiCheckout&version=1&environment=ui-test',
 'deploy-blocked':'/can-i-deploy?pacticipant=UiCheckout&version=2&environment=ui-test',
 'deploy-unknown':'/can-i-deploy?pacticipant=UiCheckout&version=3&environment=ui-test',
 'dashboard':'/dashboard/provider/UiPayments/consumer/UiCheckout',
 'explanation-extension':'/pactling/v1/can-i-deploy/explanation?pacticipant=UiCheckout&version=3&environment=ui-test',
}
accepts={'absent':None,'wildcard':'*/*','hal':'application/hal+json','json':'application/json','html':'text/html','browser':'text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8','json-preferred':'application/json;q=1,text/html;q=0.5','html-rejected':'text/html;q=0,application/hal+json;q=1'}
manifest=[];before={}
for name,path in paths.items():
 for mode,accept in accepts.items():
  status,headers,body=call(path,accept)
  suffix='html' if 'text/html' in headers.get('content-type','') else 'json' if 'json' in headers.get('content-type','') else 'txt'
  file=f'{name}.{mode}.{suffix}';(out/file).write_bytes(body)
  entry={'name':name,'path':path,'accept':accept,'mode':mode,'status':status,'contentType':headers.get('content-type'),'vary':headers.get('vary'),'location':headers.get('location'),'brokerVersion':headers.get('x-pact-broker-version'),'file':file,'sha256':hashlib.sha256(body).hexdigest()};manifest.append(entry)
  if mode in ['hal','json']:before[(name,mode)]=(status,headers.get('content-type'),body)
# Confirm browser/HTML GETs did not change the same API documents.
checks=[]
for (name,mode),expected in before.items():
 status,headers,body=call(paths[name],accepts[mode]);same=(status,headers.get('content-type'),body)==expected
 checks.append({'name':name,'mode':mode,'byteIdentical':same})
 # extension timestamps deliberately vary on pactling; baseline source-only exception recorded.
 if name!='explanation-extension':assert same,(name,mode)
(out/'negotiation.json').write_text(json.dumps(manifest,indent=2)+'\n')
(out/'api-after-html.json').write_text(json.dumps(checks,indent=2)+'\n')
for name in paths:
 print(name,[(x['mode'],x['status'],x['contentType'],x['location']) for x in manifest if x['name']==name and x['mode'] in ['absent','hal','json','html','browser']])
