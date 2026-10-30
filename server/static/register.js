(() => {
  const form = document.getElementById('registerForm');
  const status = document.getElementById('registerStatus');
  let loading;
  async function refresh() {
    const response = await fetch('/appsvc/regtoken.asp', {cache: 'no-store'});
    if (!response.ok) throw new Error('Nie można pobrać obrazka. Spróbuj za chwilę.');
    const lines = (await response.text()).trim().split(/\r?\n/);
    if (!/^[a-zA-Z0-9]{32}$/.test(lines[1])) throw new Error('Nieprawidłowa odpowiedź serwera.');
    document.getElementById('tokenid').value = lines[1];
    document.getElementById('captchaImage').src = '/appsvc/tokenpic.asp?tokenid=' + encodeURIComponent(lines[1]);
    document.getElementById('tokenval').value = '';
  }
  document.getElementById('refreshCaptcha').addEventListener('click', () => {
    loading = refresh().catch(error => { status.textContent = error.message; });
  });
  form.addEventListener('submit', async event => {
    event.preventDefault();
    const button = form.querySelector('[type=submit]'); button.disabled = true;
    try {
      await loading;
      const response = await fetch('/appsvc/fmregister3.asp', {method: 'POST', body: new URLSearchParams(new FormData(form))});
      const reply = await response.text();
      if (!response.ok || !/^reg_success:\d+$/.test(reply)) {
        const reasons = {
          'reg_failed:number_taken': 'Ten numer GG jest już zajęty. Wybierz inny lub zostaw pole puste.',
          'reg_failed:number_invalid': 'Podaj numer od 1 do 16777215 albo zostaw pole puste.',
          'reg_failed:invite': 'Kod zaproszenia jest nieprawidłowy, wyłączony lub wykorzystany już 3 razy.',
          'reg_failed:captcha': 'Kod z obrazka jest nieprawidłowy lub wygasł. Wpisz nowy kod.',
          'reg_failed:email_taken': 'Na ten adres e-mail istnieje już konto.',
          'reg_failed:capacity': 'Nie można teraz założyć kolejnego konta. Skontaktuj się z właścicielem.'
        };
        const message = response.status === 429 ? 'Zbyt wiele prób. Odczekaj minutę i spróbuj ponownie.' :
          (reasons[reply] || 'Nie udało się założyć konta. Sprawdź zaproszenie, hasło i e-mail.');
        try { await refresh(); } catch (_) { /* Keep the useful registration error. */ }
        throw new Error(message + (reply.startsWith('reg_failed') || response.status === 429 ? ' Nie zużyto użycia zaproszenia.' : ' Spróbuj ponownie za chwilę.'));
      }
      status.textContent = 'Twój numer GG: ' + reply.split(':')[1] + '. Zapisz go i użyj z wybranym hasłem w zmodyfikowanym kliencie GG 6.1.';
      form.reset(); form.hidden = true;
    } catch (error) { status.textContent = error.message; }
    finally { button.disabled = false; }
  });
  // Fetch only when registration is opened; merely visiting the homepage creates no database rows.
  document.querySelector('[onclick*="register"]').addEventListener('click', () => {
    if (!loading) loading = refresh().catch(error => { status.textContent = error.message; });
  });
})();
