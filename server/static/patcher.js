// GG-Retro Binary Patcher
// GG 6.1.0.158: custom endpoint, disabled legacy updater and correct sound defaults.

const ORIGINAL_CLIENT_SHA256 = '74b78af318c52f86dff8a7ee27a7a0b5ea545167aebb594a8a559ec97f2e3a57';

// Offsets are valid ONLY for the original executable identified by the full hash.
// Keep this separate from hostname rewriting: an arbitrary EXE must never receive
// machine-code changes merely because it contains a matching hostname.
async function patchClient(input, newDomain) {
  const data = new Uint8Array(input);
  const digest = await crypto.subtle.digest('SHA-256', data);
  const hash = Array.from(new Uint8Array(digest), byte => byte.toString(16).padStart(2, '0')).join('');
  if (hash !== ORIGINAL_CLIENT_SHA256) {
    throw new Error('Wybierz oryginalny gg.exe z instalatora GG 6.1 poniżej (wersja 6.1.0.158). Zmienionych plików i innych wersji nie patchujemy; możesz pobrać gotowy klient.');
  }
  const results = patchBinary(data, newDomain);

  // Three cdecl import thunks: UpdateNow, SetConnectionStatus and Init. The
  // caller removes arguments, so xor eax,eax; ret preserves its stack contract.
  // update.dll remains installed/imported, but none of its updater APIs is called.
  for (const offset of [0x45f80, 0x45f86, 0x45f8c]) {
    data.set([0x31, 0xc0, 0xc3, 0x90, 0x90, 0x90], offset);
  }

  // Reuse the now-unused updater URL's 32-byte, NUL-terminated storage in .rdata.
  // No sections, resource labels, file names or PE sizes need to change.
  data.fill(0, 0x841bc, 0x841dc);
  data.set(toAscii('wiadomosc\0dostepny\0'), 0x841bc);
  data.set([0x68, 0xbc, 0x41, 0x48, 0x00], 0x33207); // push "wiadomosc"
  data.set([0x68, 0xc6, 0x41, 0x48, 0x00], 0x33221); // push "dostepny"
  // Replace translated-label lookup + pop ecx with pop eax + padding. Subsequent
  // code still reads Sounds/MsgPath or OnlinePath; only the fallback basename
  // used with <installation directory>\sounds\<basename>.wav changes.
  data.set([0x58, 0x90, 0x90, 0x90, 0x90, 0x90], 0x33226);
  return { data, results };
}

// All known hostnames that need to be replaced
const HOSTNAMES = [
  'appsrv.gadu-gadu.pl',
  'appmsg.gadu-gadu.pl',    // 19 chars - hub server
  'register.gadu-gadu.pl',  // 21 chars - registration (LONGEST)
  'adserver.gadu-gadu.pl',  // 21 chars - ads
  'update.gadu-gadu.pl',    // 19 chars - auto-update
  'www.gadu-gadu.pl',       // 16 chars - website
  'pubdir.gadu-gadu.pl',    // 19 chars - public directory
  'retr.gadu-gadu.pl',      // 17 chars - retrieval
  'smsat.gadu-gadu.pl',     // 18 chars - SMS
];

const MAX_DOMAIN_LENGTH = Math.min(...HOSTNAMES.map(h => h.length)); // 16 chars

// Convert string to UTF-16LE bytes
function toUtf16Le(str) {
  const buf = new Uint8Array(str.length * 2);
  for (let i = 0; i < str.length; i++) {
    const code = str.charCodeAt(i);
    buf[i * 2] = code & 0xFF;
    buf[i * 2 + 1] = (code >> 8) & 0xFF;
  }
  return buf;
}

// Convert string to ASCII bytes
function toAscii(str) {
  const buf = new Uint8Array(str.length);
  for (let i = 0; i < str.length; i++) {
    buf[i] = str.charCodeAt(i);
  }
  return buf;
}

// Find all occurrences of pattern in data
function findAll(data, pattern) {
  const positions = [];
  for (let i = 0; i <= data.length - pattern.length; i++) {
    let match = true;
    for (let j = 0; j < pattern.length; j++) {
      if (data[i + j] !== pattern[j]) {
        match = false;
        break;
      }
    }
    if (match) positions.push(i);
  }
  return positions;
}

// Replace all occurrences (null-padded to maintain size)
function replaceAll(data, pattern, replacement) {
  const positions = findAll(data, pattern);
  if (replacement.length > pattern.length) throw new Error("Replacement exceeds original hostname length");
  const padded = new Uint8Array(pattern.length);
  padded.set(replacement);

  for (const pos of positions) {
    const width = pattern[1] === 0 ? 2 : 1;
    const suffixStart = pos + pattern.length;
    const next = data[suffixStart];
    if (replacement.length < pattern.length && (next === 47 || next === 58 || next === 63)) {
      // A hostname can be embedded in a complete URL. Shift its suffix before
      // padding; inserting NULs immediately after the domain truncates the URL.
      let end = suffixStart;
      while (end + width <= data.length && (data[end] !== 0 || (width === 2 && data[end + 1] !== 0))) end += width;
      const suffix = data.slice(suffixStart, end);
      data.set(replacement, pos);
      data.set(suffix, pos + replacement.length);
      data.fill(0, pos + replacement.length + suffix.length, end);
    } else {
      data.set(padded, pos);
    }
  }
  return positions.length;
}

// Patch binary data with new domain
function patchBinary(data, newDomain) {
  if (newDomain.length > MAX_DOMAIN_LENGTH || !/^[a-zA-Z0-9]+(?:[.-][a-zA-Z0-9]+)*$/.test(newDomain)) throw new Error(`Nieprawidłowa domena (maksymalnie ${MAX_DOMAIN_LENGTH} znaków ASCII).`);
  const results = { ascii: 0, utf16: 0 };
  const newDomainAscii = toAscii(newDomain);
  const newDomainUtf16 = toUtf16Le(newDomain);

  // Replace each hostname with the new domain
  for (const hostname of HOSTNAMES) {
    // ASCII replacement
    const asciiPattern = toAscii(hostname);
    results.ascii += replaceAll(data, asciiPattern, newDomainAscii);

    // UTF-16LE replacement
    const utf16Pattern = toUtf16Le(hostname);
    results.utf16 += replaceAll(data, utf16Pattern, newDomainUtf16);
  }

  return results;
}

// Handle patcher form submission
function patchFile(event) {
  event.preventDefault();

  const file = document.getElementById('ggFile').files[0];
  const newDomain = document.getElementById('serverAddress').value.trim();
  const statusDiv = document.getElementById('patcherStatus');

  if (!file) {
    statusDiv.innerHTML = '<div class="info-box" style="background: #FFE0E0;">Nie wybrano pliku!</div>';
    return false;
  }

  if (!newDomain) {
    statusDiv.innerHTML = '<div class="info-box" style="background: #FFE0E0;">Podaj adres serwera!</div>';
    return false;
  }

  if (newDomain.length > MAX_DOMAIN_LENGTH || !/^[a-zA-Z0-9]+(?:[.-][a-zA-Z0-9]+)*$/.test(newDomain)) {
    statusDiv.innerHTML = '<div class="info-box" style="background: #FFE0E0;">Adres serwera max ' + MAX_DOMAIN_LENGTH + ' znakow! (masz: ' + newDomain.length + ')</div>';
    return false;
  }

  statusDiv.innerHTML = '<div class="info-box">Patchowanie w toku...</div>';

  const reader = new FileReader();
  reader.onload = async function(e) {
    try {
      const { data } = await patchClient(new Uint8Array(e.target.result), newDomain);
      const blob = new Blob([data], { type: 'application/octet-stream' });
      const url = URL.createObjectURL(blob);
      const outputName = file.name.replace(/\.exe$/i, '') + '_patched.exe';

      statusDiv.replaceChildren();
      const info = document.createElement('div'); info.className = 'info-box';
      info.textContent = `Klient przygotowany dla ${newDomain}. Wyłączono stary aktualizator i poprawiono domyślne ścieżki dźwięków. `;
      const link = document.createElement('a'); link.href = url; link.download = outputName;
      link.className = 'button'; link.textContent = `Pobierz ${outputName}`;
      info.append(link); statusDiv.append(info);
    } catch (error) {
      statusDiv.textContent = error.message || 'Nie udało się przygotować klienta.';
    }
  };

  reader.onerror = function() {
    statusDiv.innerHTML = '<div class="info-box" style="background: #FFE0E0;">Blad odczytu pliku!</div>';
  };

  reader.readAsArrayBuffer(file);
  return false;
}
