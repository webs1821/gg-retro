const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync(require('node:path').join(__dirname, '../server/static/patcher.js'), 'utf8');
const scope = vm.createContext({}); vm.runInContext(source, scope);
for (const encoding of ['ascii', 'utf16le']) {
  const source = Buffer.from('before\0http://adserver.gadu-gadu.pl/getbanner.asp?x=1\0after\0', encoding);
  const data = Uint8Array.from(source);
  scope.patchBinary(data, 'gg.example.org');
  assert.equal(data.length, source.length);
  assert.ok(Buffer.from(data).toString(encoding).includes('http://gg.example.org/getbanner.asp?x=1\0'));
  assert.ok(Buffer.from(data).toString(encoding).endsWith('after\0'));
}
const untouched = Uint8Array.from(Buffer.from('appmsg.gadu-gadu.pl\0'));
assert.throws(() => scope.patchBinary(untouched, 'this-name-is-too-long.example.org'));
assert.throws(() => scope.patchBinary(untouched, '<svg/onload=x>'));
assert.equal(Buffer.from(untouched).toString(), 'appmsg.gadu-gadu.pl\0');
console.log('Patcher: domain limits, URL suffixes, ASCII/UTF-16 and binary size checks passed.');
