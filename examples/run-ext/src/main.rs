//! Load any `.empkg` and drive its tool-provider — a language-agnostic harness.
//!
//! Used to prove a non-Rust extension (the JavaScript kv extension, built with
//! jco) satisfies the exact same `emporium:extensions@0.3.0` `tool-extension`
//! contract a Rust extension does: same loader, same `list_tools` /
//! `execute_tool`, same Arrow IPC data frame decoded by the host.

use emporium::Extension;
use emporium_core::tool::Output;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).expect("usage: run-ext <path-to.empkg>");

    let ext = Extension::load(&path, serde_json::json!({})).await?;
    println!("loaded: {} v{} — {}", ext.id(), ext.version(), ext.description());
    println!("view():  {}", ext.view().await?);

    println!("\ntools:");
    for t in ext.list_tools().await? {
        println!("  {} — {}", t.id, t.name);
    }

    println!("\n-- driving the store --");
    for (k, v) in [("foo", "apple"), ("bar", "banana"), ("baz", "cherry")] {
        let out = ext
            .execute_tool("set", serde_json::json!({ "key": k, "value": v }))
            .await?;
        println!("set {k}={v} => {}", render(&out));
    }

    let got = ext.execute_tool("get", serde_json::json!({ "key": "bar" })).await?;
    println!("get bar => {}", render(&got));

    println!("\n-- list_keys (Arrow data frame, built in JS, decoded by the host) --");
    let out = ext.execute_tool("list_keys", serde_json::json!({})).await?;
    match out {
        Output::DataFrame(df) => {
            let rows = df.to_json_rows()?;
            let frame = df.to_dataframe()?;
            println!(
                "decoded {} row(s) x {} col(s) from the JS-built Arrow buffer:",
                frame.height(),
                frame.width()
            );
            for r in &rows {
                println!("  {r}");
            }
        }
        other => println!("expected a data frame, got: {}", render(&other)),
    }

    Ok(())
}

fn render(out: &Output) -> String {
    match out {
        Output::Text(t) => t.content.clone(),
        Output::DataFrame(df) => format!("<data frame, {} bytes arrow>", df.frame_ipc.len()),
        other => format!("{other:?}"),
    }
}
