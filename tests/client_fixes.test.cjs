// Validate the actual, pinned GG executable, not a synthetic instruction fixture.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const crypto = require('node:crypto');
const scope = vm.createContext({ crypto: crypto.webcrypto });
vm.runInContext(fs.readFileSync(path.join(__dirname, '../server/static/patcher.js'), 'utf8'), scope);
const original = fs.readFileSync(process.argv[2]);
const digest = b => crypto.createHash('sha256').update(b).digest('hex');

async function main() {
  assert.equal(digest(original), '74b78af318c52f86dff8a7ee27a7a0b5ea545167aebb594a8a559ec97f2e3a57');
  for (const host of ['gg.example.org', 'abcdefghijklmnop']) {
    const input = Uint8Array.from(original);
    const {data} = await scope.patchClient(input, host);
    const output = Buffer.from(data);
    assert.deepEqual(Buffer.from(input), original, 'patching must not modify caller input');
    assert.equal(output.length, original.length);
    const hostnameOnly = Uint8Array.from(original);
    scope.patchBinary(hostnameOnly, host);
    const allowed = [[0x45f80,0x45f92], [0x841bc,0x841dc],
      [0x33207,0x3320c], [0x33221,0x3322c]];
    for (let i = 0; i < output.length; i++) {
      if (!allowed.some(([start,end]) => start <= i && i < end)) {
        assert.equal(output[i], hostnameOnly[i], `unexpected change at 0x${i.toString(16)}`);
      }
    }
    // All updater IAT references formerly lived in these three cdecl thunks.
    for (const [offset,iat] of [[0x45f80,0x47a87c],[0x45f86,0x47a878],[0x45f8c,0x47a874]]) {
      const reference = Buffer.alloc(4); reference.writeUInt32LE(iat);
      assert.equal(original.subarray(0x1000,0x7a000).indexOf(reference), offset+2-0x1000);
      assert.equal(output.subarray(0x1000,0x7a000).indexOf(reference), -1);
      assert.deepEqual(output.subarray(offset,offset+6), Buffer.from('31c0c3909090','hex'));
    }
    // No change to cdecl argument cleanup or to custom path lookup/selection.
    assert.deepEqual(output.subarray(0x22bc4,0x22bd1), original.subarray(0x22bc4,0x22bd1));
    assert.deepEqual(output.subarray(0x33235,0x332bd), original.subarray(0x33235,0x332bd));
    assert.deepEqual(output.subarray(0x33226,0x3322c), Buffer.from('589090909090','hex'));
    for (const [operand,expected] of [[0x33208,'wiadomosc'],[0x33222,'dostepny']]) {
      const rva = output.readUInt32LE(operand) - 0x400000;
      assert.equal(output.toString('ascii',rva,output.indexOf(0,rva)), expected);
    }
    // Boundary after the reused URL holds an unrelated setting name.
    assert.equal(output.toString('ascii',0x841dc,0x841e2), 'Online');
    // User-facing translated resources and the PE loader/import metadata survive.
    assert.deepEqual(output.subarray(0,0x1000), original.subarray(0,0x1000));
    const names = ['Wiadomość','Dostępny'];
    for (const name of names) assert.ok(output.includes(Buffer.from(name,'utf16le')));
    for (const s of ['wiadomosc.wav','dostepny.wav']) {
      assert.ok(fs.readFileSync(process.argv[3]).includes(Buffer.from(s+'\0')),
        'sound default must name a file delivered by the original installer');
    }
    await assert.rejects(scope.patchClient(output,host), /oryginalny/);
  }
  for (const offset of [0x45f80,0x33221,0x841bc,original.length-1]) {
    const bad = Uint8Array.from(original); bad[offset] ^= 1;
    const before = Buffer.from(bad);
    await assert.rejects(scope.patchClient(bad,'gg.example.org'), /oryginalny/);
    assert.deepEqual(Buffer.from(bad),before);
  }
  for (const host of ['toolong.example.org','<svg/onload=x>']) {
    await assert.rejects(scope.patchClient(original,host), /domena/);
  }
  console.log('Real GG 6.1: updater imports disabled, cdecl cleanup preserved, sound paths match installer, boundaries/resources unchanged, unknown/modified inputs rejected.');
}
main().catch(error => { console.error(error); process.exitCode = 1; });
