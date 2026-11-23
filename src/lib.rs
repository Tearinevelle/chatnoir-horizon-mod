use std::collections::VecDeque;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use mod_api::prelude::EntitiesHost;
use mod_api::{
    self, BlockEntityDrawContext, Command, CommandArgumentValue, CommandSource, CubeFace,
    EntityRenderState, EntityRenderer, Flow, Hooks, PlayerLoggedInEvent, RegisterEntityRenderersEvent,
    RegistrationContext, RunModCommandEvent, ServerChatEvent, ServerHost, ServerTickEvent,
    ServerTickPhase, Text,
};

mod ollama;
mod persona;

use ollama::{DEFAULT_ENDPOINT, DEFAULT_MODEL};
use persona::{fallback, system_prompt, Memory, Memories};

const ENTITY_ID: &str = "chatnoir:chatnoir";
const CHAT_POINTS: i32 = 5;
const FLIRT_POINTS: i32 = 20;
const MAX_REPLY_CHARS: usize = 220;

struct NoirRenderer;

impl EntityRenderer for NoirRenderer {
    fn submit(
        &self,
        entity: &EntityRenderState,
        draw: &mut dyn BlockEntityDrawContext,
    ) -> Result<(), String> {
        draw.rotate_about([0.0, 1.0, 0.0], -entity.yaw, [0.0; 3]);
        let face = || Some(CubeFace::new("chatnoir:entity/chatnoir"));
        draw.cube([-0.3, 0.0, -0.15], [0.3, 0.75, 0.15], [face(); 6], 0xFFFFFF);
        draw.cube([-0.32, 0.75, -0.16], [0.32, 1.42, 0.16], [face(); 6], 0xFFFFFF);
        draw.cube([-0.3, 1.42, -0.15], [0.3, 1.8, 0.15], [face(); 6], 0xFFFFFF);
        Ok(())
    }
}

struct Job {
    player: u128,
    prompt: String,
    history: Vec<(bool, String)>,
    text: String,
}

#[derive(Default)]
struct App {
    memories: Memories,
    jobs: VecDeque<Job>,
    outbox: Vec<String>,
    worker_started: bool,
}

fn app() -> &'static Mutex<App> {
    static APP: OnceLock<Mutex<App>> = OnceLock::new();
    APP.get_or_init(|| Mutex::new(App::default()))
}

fn start_worker() {
    std::thread::spawn(|| loop {
        let job = {
            let mut state = app().lock().unwrap();
            state.jobs.pop_front()
        };
        let Some(job) = job else {
            std::thread::sleep(Duration::from_millis(200));
            continue;
        };
        let answer = ollama::chat(
            DEFAULT_ENDPOINT,
            DEFAULT_MODEL,
            &job.prompt,
            &job.history,
            &job.text,
        )
        .ok()
        .map(|a| a.trim().replace('\n', " "))
        .filter(|a| !a.is_empty())
        .unwrap_or_else(|| {
            let mut state = app().lock().unwrap();
            fallback(state.memories.of(job.player).stage(), &job.text).to_owned()
        });
        let answer: String = answer.chars().take(MAX_REPLY_CHARS).collect();
        let mut state = app().lock().unwrap();
        let memory = state.memories.of(job.player);
        memory.push(false, job.text);
        memory.push(true, answer.clone());
        state.outbox.push(format!("ChatNoir: {answer}"));
    });
}

fn is_flirty(text: &str) -> bool {
    let lowered = text.to_lowercase();
    [
        "люблю", "милый", "котик", "мой", "милашка", "обними", "поцел", "скучал", "красив",
        "любимый",
    ]
    .iter()
    .any(|w| lowered.contains(w))
}

#[mod_api::horizon_mod]
impl Hooks for Mod {
    fn player_logged_in(&self, host: &mut dyn ServerHost, event: &mut PlayerLoggedInEvent<'_>) {
        let dimension = host.dimension().to_owned();
        let Some(player) = host.player(event.player) else {
            return;
        };
        let Some(pos) = player.entity().position(host) else {
            return;
        };
        let at = [pos[0] + 9.0, pos[1], pos[2] + 9.0];
        if host
            .summon(&dimension, ENTITY_ID, at, Default::default())
            .is_some()
        {
            let _ = host
                .messages()
                .broadcast(Text::literal("Кот Нуар уже где-то рядом. Не оборачивайся."));
        }
    }

    fn register_entity_renderers(
        &self,
        _: &RegistrationContext,
        event: &mut RegisterEntityRenderersEvent<'_>,
    ) {
        event
            .renderers
            .register(ENTITY_ID.to_owned(), Arc::new(NoirRenderer));
    }

    fn register_commands(
        &self,
        _: &mut dyn ServerHost,
        event: &mut mod_api::RegisterCommandsEvent<'_>,
    ) {
        event.commands.push(Command::new("aiChat install"));
        event.commands.push(Command::new("aiChat check"));
        event.commands.push(Command::new("aiChat clear"));
        event.commands.push(Command::new("aiChat say <text:text>"));
        event.commands.push(Command::new("aiChat summon"));
    }

    fn run_mod_command(
        &self,
        host: &mut dyn ServerHost,
        event: &mut RunModCommandEvent<'_>,
    ) -> Flow {
        let uuid = match event.source {
            CommandSource::Player { player, .. } => player.uuid(),
            _ => return Flow::Continue,
        };
        let sub = event
            .command
            .literals
            .get(1)
            .map(String::as_str)
            .unwrap_or("");
        let arg = event
            .command
            .arguments
            .iter()
            .find_map(|a| match &a.value {
                CommandArgumentValue::String(s) => Some(s.clone()),
                _ => None,
            })
            .unwrap_or_default();
        match sub {
            "check" => {
                let line = if ollama::is_up(DEFAULT_ENDPOINT) {
                    match ollama::has_model(DEFAULT_ENDPOINT, DEFAULT_MODEL) {
                        Ok(true) => format!("Нейросеть {DEFAULT_MODEL} установлена и отвечает."),
                        Ok(false) => format!(
                            "Ollama работает, но модели {DEFAULT_MODEL} нет. Запусти /aiChat install."
                        ),
                        Err(e) => format!("Ollama ответила ошибкой: {e}"),
                    }
                } else {
                    "Ollama не запущена. Открой Ollama и повтори проверку.".to_owned()
                };
                let _ = host.messages().broadcast(Text::literal(line));
            }
            "install" => {
                if !ollama::is_up(DEFAULT_ENDPOINT) {
                    let _ = host.messages().broadcast(Text::literal(
                        "Ollama не запущена. Запусти Ollama, затем повтори /aiChat install.",
                    ));
                    return Flow::Cancel;
                }
                if matches!(
                    ollama::has_model(DEFAULT_ENDPOINT, DEFAULT_MODEL),
                    Ok(true)
                ) {
                    let _ = host.messages().broadcast(Text::literal(format!(
                        "{DEFAULT_MODEL} уже установлена. Просто пиши в чат."
                    )));
                    return Flow::Cancel;
                }
                let line = match ollama::pull(DEFAULT_ENDPOINT, DEFAULT_MODEL) {
                    Ok(()) => format!("{DEFAULT_MODEL} загружена. Просто пиши в чат."),
                    Err(e) => {
                        format!("Загрузка не удалась ({e}). Повтори /aiChat install позже.")
                    }
                };
                let _ = host.messages().broadcast(Text::literal(line));
            }
            "clear" => {
                app().lock().unwrap().memories.of(uuid).clear();
                let _ = host.messages().broadcast(Text::literal(
                    "Ты использовала катаклизм на мне..? …Ладно. Я всё забыл. Начнём заново.",
                ));
            }
            "summon" => {
                let dimension = host.dimension().to_owned();
                let line = match host.player(uuid) {
                    Some(player) => match player.entity().position(host) {
                        Some(p) => match host.summon(
                            &dimension,
                            ENTITY_ID,
                            [p[0] + 2.0, p[1], p[2]],
                            Default::default(),
                        ) {
                            Some(_) => "Кот Нуар уже рядом.".to_owned(),
                            None => "Не получилось призвать: проверь, загружен ли чанк.".to_owned(),
                        },
                        None => "Не вижу, где ты.".to_owned(),
                    },
                    None => "Не вижу тебя.".to_owned(),
                };
                let _ = host.messages().broadcast(Text::literal(line));
            }
            "say" => queue_reply(uuid, &arg),
            _ => {
                let stage = {
                    let mut state = app().lock().unwrap();
                    let memory = state.memories.of(uuid);
                    format!("{} ({} очков)", memory.stage().name(), memory.points)
                };
                let _ = host.messages().broadcast(Text::literal(format!(
                    "Кот Нуар: {stage}. install | check | clear | say <текст> | summon"
                )));
            }
        }
        Flow::Cancel
    }

    fn server_chat(&self, _: &mut dyn ServerHost, event: &mut ServerChatEvent<'_>) -> Flow {
        let text = event.raw_text.trim().to_owned();
        if !text.is_empty() {
            queue_reply(event.player, &text);
        }
        Flow::Continue
    }

    fn server_tick(&self, host: &mut dyn ServerHost, event: &mut ServerTickEvent<'_>) {
        if !matches!(event.phase, ServerTickPhase::ServerTickEnd { .. }) {
            return;
        }
        let lines: Vec<String> = {
            let mut state = app().lock().unwrap();
            std::mem::take(&mut state.outbox)
        };
        for line in lines {
            let _ = host.messages().broadcast(Text::literal(line));
        }
    }
}

fn queue_reply(player: u128, text: &str) {
    let (prompt, history) = {
        let mut state = app().lock().unwrap();
        if !state.worker_started {
            state.worker_started = true;
            start_worker();
        }
        let memory: &mut Memory = state.memories.of(player);
        memory.add_points(if is_flirty(text) {
            FLIRT_POINTS
        } else {
            CHAT_POINTS
        });
        if !ollama::is_up(DEFAULT_ENDPOINT) {
            let answer = fallback(memory.stage(), text);
            memory.push(false, text.to_owned());
            memory.push(true, answer.clone());
            state.outbox.push(format!("ChatNoir: {answer}"));
            return;
        }
        (
            system_prompt(memory.stage(), memory),
            memory.history.clone(),
        )
    };
    let mut state = app().lock().unwrap();
    state.jobs.push_back(Job {
        player,
        prompt,
        history,
        text: text.to_owned(),
    });
}

#[derive(Default)]
struct Mod;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flirty_words_score() {
        assert!(is_flirty("ты такой милый"));
        assert!(!is_flirty("где железо"));
    }
}
