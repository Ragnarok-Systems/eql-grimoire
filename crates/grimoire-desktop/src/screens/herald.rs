//! Screen: Herald. Everything the Monsters & Memories developers say in the game's Discord, kept
//! and searchable, with the question each answer was replying to.
//!
//! THE POSTS COME FROM THE RECORDER (`discord-scribe`), which writes `herald/herald.json` under the
//! app's data folder whenever it runs. This screen only reads that file: it opens no connection to
//! Discord and holds no token. With no file it says so, and says what it is waiting on.
//!
//! THE FILE IS RE-READ WHEN IT CHANGES, checked at most every few seconds by its modified time, so
//! a recorder running beside the app shows new posts without a restart.

use crate::screens::Cx;
use crate::theme::*;
use egui::{FontId, RichText, Ui};
use serde::Deserialize;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

#[derive(Clone, Debug, Deserialize)]
pub struct Reply {
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub content: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Post {
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub channel: String,
    #[serde(default)]
    pub guild: String,
    pub content: String,
    /// Milliseconds since 1970.
    pub created_at: i64,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub reply: Option<Reply>,
}

#[derive(Deserialize)]
struct File {
    #[serde(default)]
    posts: Vec<Post>,
}

/// Where the recorder writes: `<data root>/herald/herald.json`, or `GRIMOIRE_HERALD`.
pub fn herald_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("GRIMOIRE_HERALD") {
        return Some(PathBuf::from(p));
    }
    crate::updater::install::Layout::platform().map(|l| l.root().join("herald").join("herald.json"))
}

pub fn parse(text: &str) -> Result<Vec<Post>, String> {
    serde_json::from_str::<File>(text)
        .map(|f| f.posts)
        .map_err(|e| format!("herald.json could not be read: {e}"))
}

/// A post's date, in this PC's time zone.
fn when(ms: i64) -> String {
    use chrono::TimeZone;
    match chrono::Local.timestamp_millis_opt(ms) {
        chrono::LocalResult::Single(t) => t.format("%Y-%m-%d %H:%M").to_string(),
        _ => String::new(),
    }
}

#[derive(Default)]
pub struct HeraldScreen {
    posts: Vec<Post>,
    problem: Option<String>,
    loaded_mtime: Option<SystemTime>,
    checked: Option<Instant>,
    query: String,
}

impl HeraldScreen {
    fn refresh(&mut self) {
        if self
            .checked
            .is_some_and(|t| t.elapsed() < Duration::from_secs(5))
        {
            return;
        }
        self.checked = Some(Instant::now());
        let Some(path) = herald_path() else {
            self.problem = Some("This PC has no local app data folder.".to_owned());
            return;
        };
        let Ok(meta) = std::fs::metadata(&path) else {
            self.posts.clear();
            self.loaded_mtime = None;
            self.problem = None;
            return;
        };
        let mtime = meta.modified().ok();
        if mtime.is_some() && mtime == self.loaded_mtime {
            return;
        }
        match std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|t| parse(&t))
        {
            Ok(p) => {
                self.posts = p;
                self.problem = None;
                self.loaded_mtime = mtime;
            }
            Err(e) => self.problem = Some(e),
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, _cx: &mut Cx) {
        self.refresh();
        ui.ctx().request_repaint_after(Duration::from_secs(5));
        ui.label(
            RichText::new("HERALD")
                .font(crate::fonts::display(16.0))
                .color(GOLD_HI),
        );
        ui.label(
            RichText::new("What the developers say in the Monsters & Memories Discord.")
                .color(TEXT_2),
        );
        ui.add_space(8.0);
        if let Some(p) = &self.problem {
            ui.label(RichText::new(p).color(TEXT));
        }
        if self.posts.is_empty() {
            ui.label(RichText::new("No developer posts on this PC yet.").color(TEXT));
            ui.label(
                RichText::new("Herald fills from the Discord recorder. Once it runs (it needs a Discord bot token on our side), every post by a developer or staff member lands here, with the question it answered.")
                    .color(TEXT_2),
            );
            return;
        }
        ui.add(
            egui::TextEdit::singleline(&mut self.query)
                .hint_text("search developer posts")
                .desired_width(f32::INFINITY),
        );
        let q = self.query.trim().to_lowercase();
        let hits: Vec<&Post> = self
            .posts
            .iter()
            .filter(|p| {
                q.is_empty()
                    || p.content.to_lowercase().contains(&q)
                    || p.author.to_lowercase().contains(&q)
                    || p.channel.to_lowercase().contains(&q)
                    || p.reply
                        .as_ref()
                        .is_some_and(|r| r.content.to_lowercase().contains(&q))
            })
            .collect();
        ui.label(
            RichText::new(format!("{} of {} posts", hits.len(), self.posts.len()))
                .font(FontId::proportional(11.0))
                .color(TEXT_3),
        );
        ui.add_space(4.0);
        egui::ScrollArea::vertical()
            .id_salt("herald-posts")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for p in hits.iter().take(500) {
                    post(ui, p);
                    ui.add_space(10.0);
                }
                if hits.len() > 500 {
                    ui.label(
                        RichText::new("Showing the newest 500; search to narrow.").color(TEXT_3),
                    );
                }
            });
    }
}

fn post(ui: &mut Ui, p: &Post) {
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(&p.author).color(GOLD_HI).strong());
        let place = if p.guild.is_empty() {
            p.channel.clone()
        } else {
            format!("{} / {}", p.guild, p.channel)
        };
        ui.label(
            RichText::new(format!("{place}  {}", when(p.created_at)))
                .font(FontId::proportional(11.0))
                .color(TEXT_3),
        );
        if let Some(u) = &p.url {
            if ui
                .link(
                    RichText::new("open in Discord")
                        .font(FontId::proportional(11.0))
                        .color(GOLD),
                )
                .clicked()
            {
                crate::shell::open_url_quietly(u);
            }
        }
    });
    if let Some(r) = &p.reply {
        ui.label(
            RichText::new(format!("{} asked: {}", r.author, r.content))
                .italics()
                .color(TEXT_3),
        );
    }
    ui.label(RichText::new(&p.content).color(TEXT));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_recorders_file() {
        let text = r#"{"format":1,"generated_at":1,"posts":[
            {"id":"2","author":"Dev","roles":["Developer"],"guild":"MnM","channel":"general","content":"Friday.","created_at":20,"edited_at":null,"url":"https://discord.com/x","source":"chat","reply":{"author":"player","content":"next playtest?"}},
            {"id":"3","author":"Dev","content":"Patch notes.","created_at":30,"reply":null}
        ]}"#;
        let posts = parse(text).unwrap();
        assert_eq!(posts.len(), 2);
        assert_eq!(posts[0].reply.as_ref().unwrap().content, "next playtest?");
        assert!(posts[1].url.is_none());
        assert!(parse("not json").is_err());
        assert!(!when(1_791_442_352_000).is_empty());
    }
}
