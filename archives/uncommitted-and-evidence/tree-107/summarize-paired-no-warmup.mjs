import { promises as fs } from 'node:fs';
const root='D:/limo-startup-results-no-warmup';
const metrics=['firstFeedbackMs','usableWorkspaceMs','firstInteractionMs','firstCadInteractionMs','repeatedCadInteractionMs'];
const stats=(values)=>{const sorted=values.toSorted((a,b)=>a-b);return {median:sorted[Math.floor(sorted.length/2)],min:sorted[0],max:sorted.at(-1),values};};
const output={schema:1,comparisonProfile:'dev-app1-debug2-deps3-debug0',workloads:{}};
for(const workload of ['empty','document']){
const base=JSON.parse(await fs.readFile(root+'/baseline-'+workload+'-en/results.json','utf8'));
const after=JSON.parse(await fs.readFile(root+'/candidate-'+workload+'-en/results.json','utf8'));
for(const key of ['profile','cacheCondition','locale','documentSha256'])if(base[key]!==after[key])throw Error('Mismatched '+key);
if(JSON.stringify(base.hardware)!==JSON.stringify(after.hardware))throw Error('Mismatched hardware');
for(const result of [base,after])if(result.runs.length!==5||result.warmups.length!==1||[...result.warmups,...result.runs].some(run=>run.status!=='complete'||metrics.some(key=>!Number.isFinite(run[key]))))throw Error('Incomplete milestones');
const measurements={};
for(const key of metrics){const before=stats(base.runs.map(run=>run[key]));const candidate=stats(after.runs.map(run=>run[key]));measurements[key]={baseline:before,candidate,medianChangePercent:(candidate.median/before.median-1)*100};}
output.workloads[workload]={documentSha256:base.documentSha256,baselineSha256:base.executableSha256,candidateSha256:after.executableSha256,measurements,candidateCadTimings:after.runs.map(run=>run.cadTimings),baselineCadTimings:base.runs.map(run=>run.cadTimings)};
}
await fs.writeFile(root+'/summary.json',JSON.stringify(output,null,2));
console.log(JSON.stringify(output,null,2));
