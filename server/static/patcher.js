// GG-Retro Binary Patcher
// Replaces all GG server hostnames with custom endpoint

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

const MAX_DOMAIN_LENGTH = Math.min(...HOSTNAMES.map(h => h.length)); // 21 chars

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
  reader.onload = function(e) {
    const data = new Uint8Array(e.target.result);
    const originalSize = data.length;

    const results = patchBinary(data, newDomain);
    const total = results.ascii + results.utf16;

    if (total === 0) {
      statusDiv.innerHTML = '<div class="info-box" style="background: #FFE0E0;">Nie znaleziono domen do zamiany. Czy to oryginalny plik GG?</div>';
      return;
    }

    if (data.length !== originalSize) {
      statusDiv.innerHTML = '<div class="info-box" style="background: #FFE0E0;">Blad: zmieniono rozmiar pliku!</div>';
      return;
    }

    const blob = new Blob([data], { type: 'application/octet-stream' });
    const url = URL.createObjectURL(blob);
    const outputName = file.name.replace(/\.exe$/i, '') + '_patched.exe';

    statusDiv.replaceChildren();
    const info = document.createElement('div'); info.className = 'info-box';
    info.textContent = `Zmieniono ${results.ascii} adresów ASCII i ${results.utf16} UTF-16 na ${newDomain}. `;
    const link = document.createElement('a'); link.href = url; link.download = outputName;
    link.className = 'button'; link.textContent = `Pobierz ${outputName}`;
    info.append(link); statusDiv.append(info);

  };

  reader.onerror = function() {
    statusDiv.innerHTML = '<div class="info-box" style="background: #FFE0E0;">Blad odczytu pliku!</div>';
  };

  reader.readAsArrayBuffer(file);
  return false;
}
