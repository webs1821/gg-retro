// Reuse the browser patcher so downloadable clients and manual patching cannot drift.
const fs = require('node:fs'), path = require('node:path'), vm = require('node:vm');
const [input, output, hostname] = process.argv.slice(2);
if (!input || !output || !hostname) throw new Error('Usage: node patch_client.cjs input.exe output.exe hostname');
const scope = vm.createContext({});
vm.runInContext(fs.readFileSync(path.join(__dirname, '../server/static/patcher.js'), 'utf8'), scope);
const data = Uint8Array.from(fs.readFileSync(input));
const result = scope.patchBinary(data, hostname);
if (result.ascii + result.utf16 === 0) throw new Error('No original GG hostnames found');
fs.writeFileSync(output, data, {flag:'wx'});
console.log(JSON.stringify(result));
