const fs = require('fs');
if (process.env.NBCAD_STARTUP_REPORT_FILE) {
  const origin = Date.now();
  const values = [{phase:'feedback',processElapsedMs:10,webElapsedMs:5,pid:process.pid},{phase:'workspace',processElapsedMs:25,webElapsedMs:20,pid:process.pid},{phase:'interaction',processElapsedMs:30,webElapsedMs:25,durationMs:5,pid:process.pid},{phase:'cadInteraction',processElapsedMs:40,webElapsedMs:35,durationMs:10,detail:JSON.stringify({sketchCommandsMs:1,sketchFrameMs:2,solidCommandsMs:3,solidFrameMs:4,repeatedCadInteractionMs:7,repeatedTimings:{sketchCommandsMs:1,sketchFrameMs:1,solidCommandsMs:2,solidFrameMs:3}}),pid:process.pid}].map(value=>({...value,processStartedUnixMs:origin}));
  if (process.env.MOCK_STARTUP_FAILURE === 'true') values[3]={...values[3],phase:'error',detail:'mock model restoration failure'};
  fs.writeFileSync(process.env.NBCAD_STARTUP_REPORT_FILE, values.map(JSON.stringify).join('\n')+'\n');
  setInterval(() => {},1000);
}
