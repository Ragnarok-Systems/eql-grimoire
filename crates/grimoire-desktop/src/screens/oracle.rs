//! Screen: Oracle. Ask a question in plain words; the answer is written on this PC from the wiki
//! bundle, and every source is numbered and linked (to its Compendium record when there is one,
//! otherwise to the wiki page).
//!
//! THE CONSENT CARD COMES FIRST, and it is the only door to the download: it names both files,
//! their sizes, where they come from and their licences, and nothing is fetched until it is pressed.

use crate::oracle::{fetch, Cmd, Exchange, Oracle, Phase};
use crate::screens::{Ask, Cx};
use crate::theme::*;
use egui::{FontId, RichText, Ui};

#[derive(Default)]
pub struct OracleScreen {
    oracle: Oracle,
    input: String,
}

fn mb(bytes: u64) -> String {
    format!("{} MB", (bytes as f64 / 1e6).round() as u64)
}

impl OracleScreen {
    pub fn ui(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let (phase, history) = self.oracle.view();
        ui.label(
            RichText::new("ORACLE")
                .font(crate::fonts::display(16.0))
                .color(GOLD_HI),
        );
        ui.label(
            RichText::new("Ask about Monsters & Memories. Answers come from the community wiki, written on this PC.")
                .color(TEXT_2),
        );
        ui.add_space(10.0);

        match &phase {
            Phase::NeedFiles => {
                self.consent(ui);
                return;
            }
            Phase::Downloading { what, done, total } => {
                ui.label(RichText::new(format!("Downloading {what}...")).color(TEXT));
                let f = if *total > 0 {
                    *done as f32 / *total as f32
                } else {
                    0.0
                };
                ui.add(egui::ProgressBar::new(f).text(format!("{} of {}", mb(*done), mb(*total))));
                ui.label(
                    RichText::new(
                        "You can close Grimoire; the download picks up where it stopped next time.",
                    )
                    .font(FontId::proportional(11.0))
                    .color(TEXT_3),
                );
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(200));
                return;
            }
            Phase::Failed(why) => {
                ui.label(RichText::new(why).color(TEXT));
                if ui.button("Try again").clicked() {
                    self.oracle.send(Cmd::Download);
                }
                ui.add_space(8.0);
            }
            _ => {}
        }

        if cx.lore.lore.is_none() && !cx.lore.loading {
            ui.label(
                RichText::new(
                    cx.lore
                        .why
                        .unwrap_or("Oracle needs the wiki data, which is not loaded."),
                )
                .color(TEXT_2),
            );
            return;
        }

        let busy = matches!(phase, Phase::Starting | Phase::Answering);
        ui.horizontal(|ui| {
            let r = ui.add_enabled(
                !busy,
                egui::TextEdit::singleline(&mut self.input)
                    .hint_text("e.g. what level is Fallen Pass?")
                    .desired_width((ui.available_width() - 70.0).max(120.0)),
            );
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let pressed = ui.add_enabled(!busy, egui::Button::new("Ask")).clicked();
            if (enter || pressed) && !self.input.trim().is_empty() {
                self.oracle.send(Cmd::Ask(self.input.trim().to_owned()));
                self.input.clear();
            }
        });
        match phase {
            Phase::Starting => {
                ui.label(
                    RichText::new("Loading the answer model (a few seconds the first time)...")
                        .color(TEXT_3),
                );
            }
            Phase::Answering => {
                ui.label(RichText::new("Writing...").color(TEXT_3));
            }
            _ => {}
        }
        if busy {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
        ui.add_space(8.0);

        egui::ScrollArea::vertical()
            .id_salt("oracle-history")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for x in history.iter().rev() {
                    exchange(ui, cx, x);
                    ui.add_space(12.0);
                }
                if history.is_empty() {
                    ui.label(
                        RichText::new("Try: who gives A Sweet Treat? What drops the Acid-Washed Skinsuit? Which zones border Ail'Vorith?")
                            .color(TEXT_3),
                    );
                }
                ui.add_space(6.0);
                ui.label(
                    RichText::new("Answers are written by a small model from the wiki's text and can be wrong; check the sources. Wiki text is CC BY-SA 4.0.")
                        .font(FontId::proportional(11.0))
                        .color(TEXT_3),
                );
            });
    }

    fn consent(&mut self, ui: &mut Ui) {
        let have_engine = self
            .oracle
            .dirs()
            .is_some_and(|d| d.engine().join(crate::oracle::engine::SERVER_EXE).is_file());
        let mut total = 0;
        ui.label(RichText::new("Oracle needs these files before its first answer:").color(TEXT));
        ui.add_space(4.0);
        let have = |pin: &fetch::Pin| {
            self.oracle
                .dirs()
                .is_some_and(|d| fetch::have(&d.models(), pin))
        };
        let wanted = [
            (fetch::ENGINE, !have_engine),
            (fetch::MODEL, !have(&fetch::MODEL)),
            (fetch::EMBEDDER, !have(&fetch::EMBEDDER)),
        ];
        for (pin, needed) in wanted {
            if !needed {
                continue;
            }
            total += pin.bytes;
            ui.label(
                RichText::new(format!(
                    "{}, {}, from {}. Licence: {}.",
                    pin.what,
                    mb(pin.bytes),
                    pin.from,
                    pin.licence
                ))
                .color(TEXT_2),
            );
        }
        ui.add_space(4.0);
        ui.label(
            RichText::new("They are checked against pinned fingerprints, kept in Grimoire's data folder, and never leave this PC. Questions are answered offline.")
                .font(FontId::proportional(11.0))
                .color(TEXT_3),
        );
        ui.add_space(8.0);
        if ui.button(format!("Download ({})", mb(total))).clicked() {
            self.oracle.send(Cmd::Download);
        }
        ui.label(
            RichText::new("Nothing is downloaded until you press this.")
                .font(FontId::proportional(11.0))
                .color(TEXT_3),
        );
    }
}

fn exchange(ui: &mut Ui, cx: &mut Cx, x: &Exchange) {
    ui.label(RichText::new(&x.question).color(GOLD_HI).strong());
    if let Some(e) = &x.error {
        ui.label(RichText::new(e).color(TEXT_2));
        return;
    }
    if x.answer.is_empty() && !x.done {
        ui.label(RichText::new("...").color(TEXT_3));
    } else {
        ui.label(RichText::new(&x.answer).color(TEXT));
    }
    /* A decline has no sources worth listing: none of them answered. */
    if x.sources.is_empty() || x.check.as_ref().is_some_and(|c| c.declined) {
        return;
    }
    let cited = x
        .check
        .as_ref()
        .map(|c| c.cited.clone())
        .unwrap_or_default();
    ui.add_space(4.0);
    for (i, s) in x.sources.iter().enumerate() {
        let n = i + 1;
        if x.done && !cited.is_empty() && !cited.contains(&n) {
            continue;
        }
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.label(
                RichText::new(format!("[{n}]"))
                    .font(FontId::monospace(11.0))
                    .color(TEXT_3),
            );
            let label = match &s.section {
                Some(sec) => format!("{} > {sec}", s.page),
                None => s.page.clone(),
            };
            let record = cx.lore.lore.and_then(|l| l.find(&s.page));
            if let Some((kind, _)) = record {
                if ui.link(RichText::new(&label).color(GOLD)).clicked() {
                    cx.ask = Ask::ShowRecord(kind, s.page.clone());
                }
            } else if let Some(url) = &s.url {
                if ui.link(RichText::new(&label).color(GOLD)).clicked() {
                    crate::shell::open_url_quietly(url);
                }
            } else {
                ui.label(RichText::new(&label).color(TEXT_2));
            }
            if let (Some(url), Some(_)) = (&s.url, record) {
                if ui
                    .link(
                        RichText::new("wiki")
                            .font(FontId::proportional(11.0))
                            .color(TEXT_3),
                    )
                    .clicked()
                {
                    crate::shell::open_url_quietly(url);
                }
            }
        });
    }
    if x.done {
        if let Some(c) = &x.check {
            let note = if c.declined {
                Some("The wiki pages Oracle found do not answer this. Try naming the zone, NPC or item.".to_owned())
            } else if !c.invalid.is_empty() {
                let nums: Vec<String> = c.invalid.iter().map(|n| format!("[{n}]")).collect();
                Some(format!(
                    "This answer cites {}, which is not one of its sources; treat it with care.",
                    nums.join(" ")
                ))
            } else if !c.grounded {
                Some("This answer cites no sources; treat it with care.".to_owned())
            } else {
                None
            };
            if let Some(note) = note {
                ui.label(
                    RichText::new(note)
                        .font(FontId::proportional(11.0))
                        .color(TEXT_3),
                );
            }
        }
        ui.label(
            RichText::new(format!("{:.1} s", x.seconds))
                .font(FontId::proportional(10.0))
                .color(TEXT_3),
        );
    }
}
