//! 利用者の~/.claudeから稼働中セッションとバックグラウンドジョブを読み、プロセスの生存も表示する。
//! Task 14の実動作確認用。
use claude_profile_switcher::models::gateways::jobs::read_jobs;
use claude_profile_switcher::models::gateways::live_sessions::read_live_sessions;
use claude_profile_switcher::models::gateways::process::SysProcessInfo;
use claude_profile_switcher::models::ports::ProcessInfo;

fn main() {
    let dir = std::path::Path::new(&std::env::var("HOME").expect("HOME")).join(".claude");
    let p = SysProcessInfo::new();
    for s in read_live_sessions(&dir).expect("sessions") {
        println!(
            "pid={} alive={} session={} status={:?} name={:?}",
            s.pid,
            p.is_alive(s.pid),
            s.session_id,
            s.status,
            s.name
        );
    }
    for j in read_jobs(&dir).expect("jobs") {
        println!(
            "job={} state={} tasks={} detail={:?}",
            j.job_id, j.state, j.in_flight_tasks, j.detail
        );
    }
    println!("self_rss_bytes={}", p.self_rss_bytes());
}
