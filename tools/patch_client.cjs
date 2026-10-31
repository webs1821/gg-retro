// Reuse the browser patcher so downloadable clients and manual patching cannot drift.
const fs = require('node:fs'), path = require('node:path'), vm = require('node:vm');
const [input, output, hostname] = process.argv.slice(2);
if (!input || !output || !hostname) throw new Error('Usage: node patch_client.cjs input.exe output.exe hostname');
const scope = vm.createContext({ crypto: require('node:crypto').webcrypto });
vm.runInContext(fs.readFileSync(path.join(__dirname, '../server/static/patcher.js'), 'utf8'), scope);
scope.patchClient(Uint8Array.from(fs.readFileSync(input)), hostname).then(({data, results}) => {
  fs.writeFileSync(output, data, {flag:'wx'});
  console.log(JSON.stringify({...results, legacy_updater: 'disabled', sound_defaults: 'fixed'}));
}).catch(error => { console.error(error.message); process.exitCode = 1; });
