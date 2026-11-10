//! Small administrative interface, also used by the isolated integration test.
use crate::retained::History;
pub(super) fn run(args: &[String]) -> i32 {
    let result = (|| -> Result<(), String> {
        let action = args.first().map(String::as_str).unwrap_or("list");
        let pane = args.get(1).map(String::as_str).unwrap_or("");
        let mut history = History::for_pane(pane)?;
        history.reload()?;
        match action {
            "list" => println!(
                "{}",
                serde_json::to_string(
                    &history
                        .entries
                        .values()
                        .map(|e| e.to_json())
                        .collect::<Vec<_>>()
                )
                .map_err(|e| e.to_string())?
            ),
            "sync" => {
                let mut sessions = crate::tmux::query_sessions();
                let minutes =
                    crate::tmux::display_message(pane, "#{@sidebar_idle_retention_minutes}")
                        .parse()
                        .unwrap_or(30);
                history.sync(&mut sessions, crate::time::now_epoch_secs(), minutes)?;
            }
            "pin" | "resume" => {
                let key = args
                    .get(2)
                    .ok_or("history pin/resume requires a record key")?;
                let id = format!("retained:{key}");
                let now = crate::time::now_epoch_secs();
                if action == "pin" {
                    println!("{}", history.toggle_pin(&id, now)?);
                } else {
                    println!("{}", history.resume(&id, pane, now)?);
                }
            }
            _ => return Err("usage: history list|sync|pin|resume <tmux-pane> [record-key]".into()),
        }
        Ok(())
    })();
    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}
