const fs = require('node:fs');
const path = require('node:path');
const cp = require('node:child_process');
const repo=path.resolve(__dirname,'../../../..');
const source=fs.readFileSync(path.join(repo,'src-tauri/src/lib.rs'),'utf8');
const scratch=fs.mkdtempSync(path.join(require('node:os').tmpdir(),'dicta-regressions-'));
function extract(start,end) { return source.slice(source.indexOf(start),source.indexOf(end,source.indexOf(start))); }
const paste=extract('fn auto_paste_text(', '\nstruct AppState');
const compact=extract('fn compact_queue_error(', '\n/// Estimate cost');
const harness=String.raw`
use std::time::Duration;
use std::cell::RefCell;
#[derive(Default)] struct FakeState { text: String, image: bool, ctrl: bool, fail_v: bool }
thread_local! { static ENV: RefCell<FakeState> = RefCell::new(FakeState::default()); }
struct AppHandle;
struct Clipboard;
impl AppHandle { fn clipboard(&self)->Clipboard { Clipboard } }
impl Clipboard {
    fn read_text(&self)->Result<String,String> { ENV.with(|s| {let s=s.borrow();if s.image {Err("not text".into())}else{Ok(s.text.clone())}}) }
    fn write_text(&self,text:&str)->Result<(),String> { ENV.with(|s|{let mut s=s.borrow_mut();s.text=text.into();s.image=false;});Ok(()) }
}
mod enigo {
    use super::ENV;
    #[derive(Default)] pub struct Settings;
    pub struct Enigo;
    pub enum Key { Control, Unicode(char) }
    pub enum Direction { Press, Click, Release }
    impl Enigo {
        pub fn new(_: &Settings)->Result<Self,String> {Ok(Self)}
        pub fn key(&mut self,key:Key,dir:Direction)->Result<(),String> {
            ENV.with(|s|{let mut s=s.borrow_mut();match (key,dir) {
                (Key::Control,Direction::Press)=>s.ctrl=true,
                (Key::Control,Direction::Release)=>s.ctrl=false,
                (Key::Unicode('v'),Direction::Click) if s.fail_v=>return Err("injected V failure".into()),
                _=>{}
            } Ok(())})
        }
    }
    // Models the checked enigo 0.2.1 default release_keys_when_dropped=true.
    // This fake does not prove that a native Release succeeds in Windows.
    impl Drop for Enigo {
        fn drop(&mut self) { ENV.with(|s|s.borrow_mut().ctrl=false); }
    }
}
use enigo::{Enigo,Key,Settings};
`+paste+compact+String.raw`
#[test] fn delivery_without_target_must_not_report_success() {
    ENV.with(|s|s.borrow_mut().text="original".into());
    let result=auto_paste_text(&AppHandle,"transcription");
    assert!(result.is_err(),"reproduced: native input returned Ok but fake target received nothing and function reported success");
}
#[test] fn keyboard_cleanup_depends_on_enigo_drop() {
    ENV.with(|s|{s.borrow_mut().fail_v=true;});
    assert!(auto_paste_text(&AppHandle,"transcription").is_err());
    assert!(!ENV.with(|s|s.borrow().ctrl),"model of dependency Drop should release synthetic Ctrl");
}
#[test] fn non_text_clipboard_must_not_be_replaced_with_empty_text() {
    ENV.with(|s|s.borrow_mut().image=true);
    let _=auto_paste_text(&AppHandle,"transcription");
    assert!(ENV.with(|s|s.borrow().image),"reproduced: original image could not be read as text and restore erased it");
}
#[test] fn queue_error_with_unicode_must_not_panic() {
    let error=format!("{}é more text","a".repeat(239));
    assert!(std::panic::catch_unwind(||compact_queue_error(&error)).is_ok(),"reproduced: byte 240 splits a UTF-8 character");
}
`;
const rs=path.join(scratch,'paste_regressions.rs');
const exe=path.join(scratch,'paste_regressions.exe');
fs.writeFileSync(rs,harness);
let r=cp.spawnSync('rustc',['--test','--edition','2021',rs,'-o',exe],{encoding:'utf8'});
if(r.status!==0){console.log(r.stderr);process.exit(r.status||1)}
r=cp.spawnSync(exe,['--test-threads=1'],{encoding:'utf8'});
const output=r.stdout+r.stderr;
fs.writeFileSync(path.join(scratch,'baseline-regressions.txt'),output);
console.log(output);
console.log('Expected reproduction exit:',r.status);
process.exitCode=r.status;
