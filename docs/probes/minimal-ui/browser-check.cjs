const { chromium }=require('playwright-core');
const fs=require('fs');
(async()=>{
 const out=process.argv[2];fs.mkdirSync(out,{recursive:true});
 const browser=await chromium.launch({executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',headless:true});
 const context=await browser.newContext({javaScriptEnabled:false});
 const page=await context.newPage();const network=[];
 page.on('request',r=>network.push(r.url()));
 const paths={index:'/ui/',pact:'/ui/pacts/provider/UiPayments/consumer/UiCheckout/version/1',matrix:'/ui/matrix/provider/UiPayments/consumer/UiCheckout',explanation:'/ui/deployment-explanation?pacticipant=UiCheckout&version=3&environment=ui-test'};
 const checks=[];
 for(const [width,height] of [[1280,900],[390,844]]){
  await page.setViewportSize({width,height});
  for(const [name,path] of Object.entries(paths)){
   const r=await page.goto('http://localhost:9493'+path,{waitUntil:'networkidle'});
   if(r.status()!==200)throw Error(`${path}: ${r.status()}`);
   if(name==='matrix' && width===390){const cell=await page.locator('th').first().boundingBox();if(cell.width<80)throw Error('matrix columns too narrow to read: '+cell.width);}
   const html=await page.content();if(html.includes('<script'))throw Error('script present');
   await page.keyboard.press('Tab');const focused=await page.locator(':focus').getAttribute('href');
   if(!focused)throw Error('keyboard navigation has no link');
   await page.screenshot({path:`${out}/${name}-${width}.png`,fullPage:true});
   checks.push({name,width,status:r.status(),keyboardFocusedHref:focused,title:await page.title()});
  }
 }
 if(network.some(u=>!u.startsWith('http://localhost:9493/')))throw Error('external asset requested');
 fs.writeFileSync(out+'/browser-checks.json',JSON.stringify({javascriptEnabled:false,checks,requests:[...new Set(network)]},null,2)+'\n');
 await browser.close();console.log('8 JS-disabled page/viewport screenshots; keyboard links and local-only requests verified');
})();
