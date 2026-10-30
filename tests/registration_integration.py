"""Registration races and persistence against a real server, on a temporary database only."""
import concurrent.futures, sqlite3, urllib.error
import wire_integration as w

CODES = [f'synthetic-registration-test-code-{i:03}' for i in range(12)]
def invitations(codes=CODES):
    (w.RUN/'invite.txt').write_text('\n'.join(codes)+'\n')
def form(email, code, number=None):
    token=w.http('/appsvc/regtoken.asp').splitlines()[1]
    answer=w.dbquery('SELECT captcha_code FROM tokens WHERE token_id=?',(token,))[0][0]
    result=dict(email=email,pwd=w.PASSWORD,invite=code,tokenid=token,tokenval=answer)
    if number is not None: result['requested_uin']=str(number)
    return result
def submit(data):
    try:return w.http('/appsvc/fmregister3.asp',data)
    except urllib.error.HTTPError as error:return f'HTTP_{error.code}'
def register(email, code, number=None):return submit(form(email,code,number))
def usage():return w.dbquery('SELECT sum(used) FROM registration_invites')[0][0]
def cursor():return w.dbquery('SELECT next_uin FROM registration_state')[0][0]
def parallel(forms):
    with concurrent.futures.ThreadPoolExecutor(max_workers=12) as pool:return list(pool.map(submit,forms))
def main():
    invitations();w.start()
    # Existing accounts do not alter the new allocation sequence.
    assert register('existing@example.invalid',CODES[0],1400000)=='reg_success:1400000'
    assert register('old-number@example.invalid',CODES[0],42)=='reg_success:42'
    assert register('auto@example.invalid',CODES[0],'')=='reg_success:112233'
    w.record('manual_low_number_and_first_auto_112233',cursor()==112234)
    w.record('fourth_registration_rejected',register('fourth@example.invalid',CODES[0])=='reg_failed:invite' and usage()==3 and cursor()==112234)
    before=usage()
    assert register('duplicate-number@example.invalid',CODES[1],42)=='reg_failed:number_taken'
    assert register('auto@example.invalid',CODES[1])=='reg_failed:email_taken'
    for bad in ('0','-1','16777216','4294967296','1e6','123.5','+123','letters'):
        assert register(f'invalid-{bad}@example.invalid',CODES[1],bad)=='reg_failed:number_invalid'
    bad=form('captcha@example.invalid',CODES[1]);bad['tokenval']='!!!!'
    assert submit(bad)=='reg_failed:captcha'
    w.record('failed_attempts_refund_invitation_and_sequence',usage()==before and cursor()==112234)
    assert register('next-taken@example.invalid',CODES[1],112234)=='reg_success:112234'
    assert register('auto-skip@example.invalid',CODES[1])=='reg_success:112235'
    assert register('maximum@example.invalid',CODES[1],16777215)=='reg_success:16777215'
    w.record('auto_skips_taken_numbers_and_ignores_manual_high_numbers',cursor()==112236)
    replies=parallel([form(f'quota-race-{i}@example.invalid',CODES[2]) for i in range(12)])
    winners=[r for r in replies if r.startswith('reg_success:')]
    w.record('concurrent_invitation_has_exactly_three_uses',len(winners)==3 and replies.count('reg_failed:invite')==9 and len(set(winners))==3)
    before=usage();old_cursor=cursor()
    replies=parallel([form(f'number-race-{i}@example.invalid',CODES[3+i%2],77777) for i in range(12)])
    w.record('concurrent_custom_number_has_one_owner',replies.count('reg_success:77777')==1 and replies.count('reg_failed:number_taken')==11 and usage()==before+1 and cursor()==old_cursor)
    before=usage();shared=form('captcha-a@example.invalid',CODES[5]);other=dict(shared,email='captcha-b@example.invalid')
    replies=parallel([shared,other]);w.record('concurrent_captcha_cannot_be_reused',sum(r.startswith('reg_success:') for r in replies)==1 and replies.count('reg_failed:captcha')==1 and usage()==before+1)
    before=usage();old_cursor=cursor()
    replies=parallel([form('same-email@example.invalid',CODES[6]),form('same-email@example.invalid',CODES[7])])
    w.record('concurrent_email_does_not_burn_number_or_code',sum(r.startswith('reg_success:') for r in replies)==1 and replies.count('reg_failed:email_taken')==1 and usage()==before+1 and cursor()==old_cursor+1)
    before=usage();old_cursor=cursor();failure=form('forced-failure@example.invalid',CODES[8])
    w.dbquery("CREATE TRIGGER test_insert_failure BEFORE INSERT ON users WHEN NEW.email='forced-failure@example.invalid' BEGIN SELECT RAISE(ABORT,'synthetic insertion failure'); END")
    assert submit(failure)=='HTTP_500'
    w.record('sql_failure_rolls_back_code_captcha_and_number',usage()==before and cursor()==old_cursor and w.dbquery('SELECT used_at FROM tokens WHERE token_id=?',(failure['tokenid'],))[0][0] is None)
    w.dbquery('DROP TRIGGER test_insert_failure')
    assert submit(failure).startswith('reg_success:')
    before=usage();old_cursor=cursor();w.stop();w.start()
    w.record('restart_preserves_usage_and_next_number',usage()==before and cursor()==old_cursor and register('still-fourth@example.invalid',CODES[0])=='reg_failed:invite')
    w.dbquery('DELETE FROM users WHERE uin=42')
    w.record('account_deletion_does_not_refund_invite',register('after-deletion@example.invalid',CODES[0],42)=='reg_failed:invite')
    w.stop();invitations([c for c in CODES if c!=CODES[8]]);w.start()
    assert register('revoked@example.invalid',CODES[8])=='reg_failed:invite'
    w.stop();invitations();w.start()
    w.record('revocation_and_readdition_preserve_consumed_credits',usage()==before and cursor()==old_cursor)
    stored=w.dbquery('SELECT code_hash,length(code_hash) FROM registration_invites')
    w.record('database_stores_only_invitation_hashes',all(size==32 for _,size in stored) and CODES[0].encode() not in (w.DB.read_bytes()))
    w.stop();invitations([]);w.start()
    assert register('closed@example.invalid',CODES[9])=='reg_failed:invite'
    client=w.Client(112233);client.close()
    w.record('empty_invitation_file_closes_registration_without_breaking_login',True)
    w.stop();invitations([CODES[0],CODES[0]])
    failed=False
    try:w.start()
    except RuntimeError:failed=True
    w.record('duplicate_invitation_file_fails_closed',failed)
    print(f'{len(w.RESULTS)} registration integration checks passed.')
if __name__=='__main__':
    try:main()
    finally:w.stop();w.LOG.close()
