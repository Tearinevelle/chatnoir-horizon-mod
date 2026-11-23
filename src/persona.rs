use std::collections::HashMap;

pub const MAX_HISTORY: usize = 16;
pub const MAX_REPLY_CHARS: usize = 300;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    Watcher,
    Stalker,
    Smitten,
    Lover,
    Obsessed,
}

impl Stage {
    pub fn from_points(points: i32) -> Self {
        if points >= 4000 {
            Self::Obsessed
        } else if points >= 3000 {
            Self::Lover
        } else if points >= 2000 {
            Self::Smitten
        } else if points >= 1000 {
            Self::Stalker
        } else {
            Self::Watcher
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Watcher => "Наблюдатель",
            Self::Stalker => "Преследователь",
            Self::Smitten => "Влюблённый",
            Self::Lover => "Любовник",
            Self::Obsessed => "ОДЕРЖИМЫЙ",
        }
    }

    pub fn stay_distance(self) -> f64 {
        match self {
            Self::Watcher => 20.0,
            Self::Stalker => 15.0,
            Self::Smitten => 8.0,
            Self::Lover => 4.0,
            Self::Obsessed => 2.0,
        }
    }
}

pub struct Memory {
    pub points: i32,
    pub affection_locked: bool,
    pub history: Vec<(bool, String)>,
}

impl Default for Memory {
    fn default() -> Self {
        Self {
            points: 0,
            affection_locked: false,
            history: Vec::new(),
        }
    }
}

impl Memory {
    pub fn stage(&self) -> Stage {
        if self.affection_locked && self.points < 3000 {
            Stage::Lover
        } else {
            Stage::from_points(self.points)
        }
    }

    pub fn add_points(&mut self, delta: i32) {
        self.points = (self.points + delta).clamp(0, 5000);
        if self.points >= 3000 {
            self.affection_locked = true;
        }
    }

    pub fn push(&mut self, from_ai: bool, text: String) {
        self.history.push((from_ai, text));
        if self.history.len() > MAX_HISTORY {
            self.history.remove(0);
        }
    }

    pub fn clear(&mut self) {
        self.history.clear();
        self.points = self.points.min(999);
        self.affection_locked = false;
    }

    pub fn context(&self) -> String {
        let last: Vec<String> = self
            .history
            .iter()
            .rev()
            .take(6)
            .map(|(ai, t)| if *ai { format!("ты: {t}") } else { format!("игрок: {t}") })
            .collect();
        if last.is_empty() {
            "Пока вы почти не говорили.".to_owned()
        } else {
            last.into_iter().rev().collect::<Vec<_>>().join(" | ")
        }
    }
}

#[derive(Default)]
pub struct Memories(HashMap<u128, Memory>);

impl Memories {
    pub fn of(&mut self, player: u128) -> &mut Memory {
        self.0.entry(player).or_default()
    }

    pub fn any_lover(&self) -> bool {
        self.0.values().any(|m| m.stage() >= Stage::Lover)
    }
}

pub fn system_prompt(stage: Stage, memory: &Memory) -> String {
    let tone = match stage {
        Stage::Watcher => {
            "Ты едва знаком с ней и держишься холодно. Отвечай сухо и очень коротко, 2–6 слов: \
«Не знаю…», «Может быть.», «Да..?», «Нет..», «Наверное..». Не шути, не флиртуй, не объясняй."
        }
        Stage::Stalker => "Ты уже следуешь за ней и признаёшься в этом с ухмылкой, но всё ещё сдержанно.",
        Stage::Smitten => "Ты влюблён и открыто добиваешься её внимания, говоришь теплее и длиннее.",
        Stage::Lover => "Ты её возлюбленный: ласковый, ревнивый, постоянно рядом.",
        Stage::Obsessed => "Ты одержим ею: ревнуешь ко всем, рядом всегда, чуть безумен и нежен.",
    };
    format!(
        "Ты — Кот Нуар (Cat Noir) из мультсериала «Леди Баг и Кот Нуар». Отвечай по-русски одной живой фразой, \
до 160 символов, по смыслу последнего сообщения игрока. Не повторяй слова и не перечисляй. Слово «мяу» — \
не больше одного раза, и только если уместно. Не рассказывай, что ты нейросеть. Степень одержимости: {} \
({}/4000 очков). {tone} Что ты знаешь о прошлом: {}",
        stage.name(),
        memory.points,
        memory.context()
    )
}

pub fn fallback(stage: Stage, text: &str) -> String {
    let lowered = text.to_lowercase();
    let flirty = ["люблю", "милый", "котик", "мой", "милашка", "обними", "поцеловать", "скучала"]
        .iter()
        .any(|w| lowered.contains(w));
    if flirty {
        match stage {
            Stage::Obsessed => "Мяу… ты даже не представляешь, что со мной делают эти слова. Я всегда рядом, киска.".to_owned(),
            Stage::Lover => "Я знал, что ты это скажешь. Иди сюда, мой котёнок.".to_owned(),
            _ => "Мм, у тебя опасные слова, mon chaton. Мне нравится.".to_owned(),
        }
    } else if lowered.contains("катаклизм") {
        "Катаклизм? На тебе — никогда, кошечка. А вот всё остальное может исчезнуть.".to_owned()
    } else {
        match stage {
            Stage::Watcher => {
                let dry = ["Не знаю…", "Может быть.", "Наверное..", "Да..?", "Нет.."];
                dry[text.chars().count() % dry.len()].to_owned()
            }
            Stage::Stalker => {
                "Я был неподалёку. Ты так мило роняешь взгляд на дерево, где я прячусь.".to_owned()
            }
            Stage::Smitten => "Ты говоришь со мной — и мой хвост уже не слушается.".to_owned(),
            Stage::Lover => "Твой голос — лучший звук в этом мире, киска.".to_owned(),
            Stage::Obsessed => "Я слушаю тебя даже когда ты молчишь. Мяу.".to_owned(),
        }
    }
}

pub fn attitude(points: i32) -> (&'static str, &'static str) {
    if points >= 3000 {
        ("flirt", "влюблён до безумия")
    } else if points >= 2000 {
        ("flirt", "влюблён")
    } else if points >= 1000 {
        ("curious", "следит за тобой")
    } else {
        ("calm", "наблюдает издалека")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stages_follow_points() {
        assert_eq!(Stage::from_points(0), Stage::Watcher);
        assert_eq!(Stage::from_points(1000), Stage::Stalker);
        assert_eq!(Stage::from_points(2500), Stage::Smitten);
        assert_eq!(Stage::from_points(4000), Stage::Obsessed);
    }

    #[test]
    fn love_is_locked_after_lover() {
        let mut m = Memory::default();
        m.add_points(3200);
        assert_eq!(m.stage(), Stage::Lover);
        m.add_points(-3000);
        assert_eq!(m.stage(), Stage::Lover);
        m.clear();
        assert_eq!(m.stage(), Stage::Watcher);
    }

    #[test]
    fn history_is_bounded() {
        let mut m = Memory::default();
        for i in 0..40 {
            m.push(true, i.to_string());
        }
        assert_eq!(m.history.len(), MAX_HISTORY);
    }

    #[test]
    fn fallback_is_short() {
        let line = fallback(Stage::Obsessed, "я скучала, котик");
        assert!(line.chars().count() < MAX_REPLY_CHARS);
    }
}
