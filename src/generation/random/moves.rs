//! Ordered Champions movepool culling and STAB/role enforcement.

use super::{
    DATA,
    set::{self, BuildState, Counter, HAZARDS, NO_STAB, PRIORITY_POKEMON, RECOVERY},
};
use crate::{
    dex::Category,
    generation::{index, sample},
    rng::Rng,
};

const PHYSICAL: &[&str] = &[
    "bellydrum",
    "bulkup",
    "coil",
    "curse",
    "dragondance",
    "fellstinger",
    "honeclaws",
    "howl",
    "meditate",
    "poweruppunch",
    "swordsdance",
    "tidyup",
    "victorydance",
];
const SPEED_SETUP: &[&str] = &[
    "agility",
    "autotomize",
    "flamecharge",
    "raindance",
    "rockpolish",
    "snowscape",
    "sunnyday",
    "trailblaze",
];
const SETUP: &[&str] = &[
    "acidarmor",
    "agility",
    "autotomize",
    "bellydrum",
    "bulkup",
    "calmmind",
    "clangoroussoul",
    "coil",
    "cosmicpower",
    "curse",
    "dragondance",
    "fellstinger",
    "flamecharge",
    "growth",
    "honeclaws",
    "howl",
    "irondefense",
    "meditate",
    "nastyplot",
    "noretreat",
    "poweruppunch",
    "quiverdance",
    "raindance",
    "rockpolish",
    "shellsmash",
    "shelter",
    "shiftgear",
    "sunnyday",
    "swordsdance",
    "tailglow",
    "takeheart",
    "tidyup",
    "trailblaze",
    "workup",
    "victorydance",
];
const SPEED_CONTROL: &[&str] = &[
    "electroweb",
    "glare",
    "icywind",
    "nuzzle",
    "quash",
    "tailwind",
    "thunderwave",
    "trickroom",
];
const PROTECT: &[&str] = &[
    "banefulbunker",
    "burningbulwark",
    "detect",
    "kingsshield",
    "protect",
    "silktrap",
    "spikyshield",
];
const PIVOT: &[&str] = &[
    "chillyreception",
    "flipturn",
    "partingshot",
    "shedtail",
    "teleport",
    "uturn",
    "voltswitch",
];
const STATUS_INFLICTING: &[&str] = &["nuzzle", "thunderwave", "toxic", "willowisp", "yawn"];
const PAIRS: &[(&str, &str)] = &[
    ("sleeptalk", "rest"),
    ("protect", "wish"),
    ("leechseed", "substitute"),
    ("reflect", "lightscreen"),
];

struct Selector<'a> {
    state: &'a BuildState<'a>,
    pool: Vec<&'static str>,
    moves: Vec<&'static str>,
    counts: Counter,
    rng: &'a mut Rng,
}

pub(super) fn choose(state: &BuildState<'_>, rng: &mut Rng) -> Vec<&'static str> {
    let mut selector = Selector {
        state,
        pool: state.pool.iter().map(String::as_str).collect(),
        moves: Vec::with_capacity(state.max_moves),
        counts: Counter::default(),
        rng,
    };
    selector.construct();
    selector.moves
}

fn remove(pool: &mut Vec<&'static str>, id: &str) -> bool {
    if let Some(i) = pool.iter().position(|&m| m == id) {
        pool.swap_remove(i);
        true
    } else {
        false
    }
}

impl Selector<'_> {
    fn enough(&self) -> bool {
        self.moves.len() + self.pool.len() <= self.state.max_moves
    }

    fn add(&mut self, id: &'static str) {
        assert!(
            remove(&mut self.pool, id),
            "tried to add {id} outside the remaining pool"
        );
        if !self.moves.contains(&id) {
            self.moves.push(id);
        }
        self.counts = Counter::measure(self.state, &self.moves);
        self.cull();
    }

    fn force(&mut self, id: &'static str) {
        if self.pool.contains(&id) {
            self.add(id);
        }
    }

    fn pick(&mut self, predicate: impl Fn(&str) -> bool) -> bool {
        let eligible: Vec<_> = self
            .pool
            .iter()
            .copied()
            .filter(|id| predicate(id))
            .collect();
        if eligible.is_empty() {
            return false;
        }
        let id = sample(self.rng, &eligible);
        self.add(id);
        true
    }

    fn incompat(&mut self, a: &[&str], b: &[&str]) {
        if self.enough() {
            return;
        }
        for &picked in &self.moves {
            for (test, prune) in [(b, a), (a, b)] {
                if test.contains(&picked) {
                    for &other in prune {
                        if other != picked
                            && remove(&mut self.pool, other)
                            && self.moves.len() + self.pool.len() <= self.state.max_moves
                        {
                            return;
                        }
                    }
                }
            }
        }
    }

    fn has_type(&self, ty: &str) -> bool {
        self.state.candidate.species.types.iter().any(|t| t == ty)
    }

    fn cull(&mut self) {
        if self.enough() {
            return;
        }
        let missing = self.state.max_moves.saturating_sub(self.moves.len());
        if missing == 2 {
            let mut unpaired = self.pool.clone();
            for &(a, b) in PAIRS {
                if self.pool.contains(&a) && self.pool.contains(&b) {
                    remove(&mut unpaired, a);
                    remove(&mut unpaired, b);
                }
            }
            if let [id] = unpaired.as_slice() {
                remove(&mut self.pool, id);
            }
        }
        if missing == 1 {
            for &(a, b) in PAIRS {
                if self.pool.contains(&a) && self.pool.contains(&b) {
                    remove(&mut self.pool, a);
                    remove(&mut self.pool, b);
                }
            }
        }
        let d = self.state.details;
        for (blocked, ids) in [
            (d.sticky_web != 0, &["stickyweb"][..]),
            (d.stealth_rock != 0, &["stealthrock"]),
            (d.defog != 0 || d.rapid_spin != 0, &["defog", "rapidspin"]),
            (d.toxic_spikes != 0, &["toxicspikes"]),
            (d.spikes >= 2, &["spikes"]),
            (d.status_cure != 0, &["healbell"]),
        ] {
            if blocked {
                for id in ids {
                    remove(&mut self.pool, id);
                }
                if self.enough() {
                    return;
                }
            }
        }
        if self.state.doubles {
            for (a, b) in [
                (SPEED_CONTROL, SPEED_CONTROL),
                (SETUP, &["fakeout", "helpinghand"]),
                (RECOVERY, &["healpulse", "lifedew"]),
                (&["healpulse"], &["lifedew"]),
                (&["coaching"], &["helpinghand"]),
            ] {
                self.incompat(a, b);
            }
            if !matches!(
                self.state.role,
                "Offensive Protect" | "Doubles Fast Attacker"
            ) {
                self.incompat(PROTECT, &["uturn"]);
            }
        }
        if !self.enough()
            && self.moves.iter().any(|&m| {
                set::move_ref(m).1.category == Category::Status
                    || matches!(m, "healingwish" | "switcheroo" | "trick")
            })
        {
            let status_moves: Vec<_> = DATA.status_moves.iter().map(String::as_str).collect();
            self.incompat(&status_moves, &["healingwish", "switcheroo", "trick"]);
        }
        for (a, b) in [
            (SETUP, PIVOT),
            (SETUP, HAZARDS),
            (SETUP, &["defog", "haze", "toxic"]),
            (PHYSICAL, PHYSICAL),
            (SPEED_SETUP, &["quickattack"]),
            (&["curse"], &["rapidspin"]),
            (&["defog"], HAZARDS),
            (&["uturn"], &["trick"]),
            (&["substitute"], PIVOT),
            (&["psychic", "psychicnoise"], &["psyshock", "psychicnoise"]),
            (&["scald", "surf", "waterfall"], &["hydropump"]),
            (&["muddywater"], &["weatherball"]),
            (
                &["gigadrain", "hornleech", "tropkick"],
                &["leafstorm", "powerwhip", "woodhammer"],
            ),
            (
                &["dazzlinggleam"],
                &["alluringvoice", "moonblast", "playrough"],
            ),
            (
                &["fireblast", "flamethrower", "weatherball"],
                &["fierydance", "heatwave", "overheat"],
            ),
            (&["aurasphere"], &["focusblast"]),
            (&["closecombat"], &["drainpunch"]),
            (&["dragonpulse", "ficklebeam"], &["dracometeor"]),
            (&["risingvoltage"], &["volttackle"]),
            (&["bodyslam"], &["doubleedge"]),
            (&["gunkshot"], &["sludgewave"]),
            (&["rockslide"], &["stoneedge"]),
            (&["foulplay"], &["knockoff"]),
            (&["taunt", "clearsmog"], &["encore"]),
            (&["roar"], &["yawn"]),
            (STATUS_INFLICTING, &["toxicspikes"]),
            (STATUS_INFLICTING, STATUS_INFLICTING),
        ] {
            self.incompat(a, b);
        }
        if !self.has_type("Dark") && self.state.preferred != "Dark" {
            self.incompat(&["knockoff"], &["suckerpunch"]);
        }
        if !self.has_type("Ice") && self.state.preferred != "Ice" {
            self.incompat(&["icebeam"], &["icywind"]);
        }
        if !self.state.doubles {
            match self.state.candidate.recipe.id.as_str() {
                "cameruptmega" => self.incompat(&["ancientpower"], &["willowisp"]),
                "rillaboom" => self.incompat(&["highhorsepower"], &["knockoff"]),
                _ => {}
            }
        }
    }

    fn required_stab(&self, ty: &str) -> bool {
        if self.counts.type_count(ty) != 0 {
            return false;
        }
        let species = self.state.candidate.species;
        match ty {
            "Bug" => !(self.has_type("Fire") || self.has_type("Steel")),
            "Grass" => {
                species.base_stats.atk >= 100
                    || self.pool.contains(&"leafstorm")
                    || self.has_type("Ghost")
                    || self.state.abilities.iter().any(|a| a == "Seed Sower")
            }
            "Normal" => {
                self.pool.contains(&"boomburst")
                    || ["Electric", "Fire", "Ghost", "Grass", "Ground"]
                        .iter()
                        .any(|ty| self.has_type(ty))
                    || species.base_species == "Squawkabilly"
            }
            "Psychic" => {
                !(self.has_type("Dark")
                    || self.has_type("Ice")
                    || (!self.state.doubles && (self.has_type("Water") || self.has_type("Steel"))))
            }
            "Rock" => species.base_stats.atk >= 80,
            "Steel" => species.base_stats.atk >= 60,
            "Dark" | "Dragon" | "Electric" | "Fairy" | "Fighting" | "Fire" | "Flying" | "Ghost"
            | "Ground" | "Ice" | "Poison" | "Water" => true,
            _ => false,
        }
    }

    fn construct(&mut self) {
        self.cull();
        if self.pool.len() <= self.state.max_moves {
            while !self.pool.is_empty() {
                let id = sample(self.rng, &self.pool);
                self.add(id);
            }
            return;
        }
        let state = self.state;
        let species = state.candidate.species;
        let id = state.candidate.recipe.id.as_str();
        let role = state.role;
        let has_type = |ty: &str| species.types.iter().any(|t| t == ty);
        let stab_candidate = |id: &str, allowed_type: &dyn Fn(&str) -> bool| {
            let (_, m, _) = set::move_ref(id);
            !NO_STAB.contains(&id)
                && set::base_power(m)
                && allowed_type(set::move_type(state, id, m))
        };
        if let Some(move_id) = state.candidate.recipe.required_move.as_deref() {
            self.add(move_id);
        }
        for id in [
            "auroraveil",
            "blizzard",
            "courtchange",
            "revivalblessing",
            "stickyweb",
        ] {
            self.force(id);
        }
        let d = state.details;
        if role == "Bulky Support" && d.defog == 0 && d.rapid_spin == 0 {
            self.force("rapidspin");
            self.force("defog");
        }
        if species.types.len() == 1 && (has_type("Normal") || has_type("Fighting")) {
            self.force("knockoff");
        }
        if self
            .pool
            .iter()
            .any(|&m| matches!(m, "irondefense" | "shelter"))
        {
            self.force("bodypress");
        }
        if d.stealth_rock == 0
            && self
                .pool
                .iter()
                .any(|&m| matches!(m, "dragontail" | "roar" | "whirlwind"))
        {
            self.force("stealthrock");
        }
        if species.base_species == "Sharpedo" {
            self.force("protect");
        }
        if id == "aegislash" && role == "Bulky Attacker" {
            self.force("kingsshield");
        }
        if id == "qwilfish" {
            self.force("flipturn");
        }
        if matches!(role, "Wallbreaker" | "Doubles Wallbreaker") || PRIORITY_POKEMON.contains(&id) {
            self.pick(|id| {
                let (_, m, _) = set::move_ref(id);
                m.priority > 0 && set::base_power(m) && has_type(set::move_type(state, id, m))
            });
        }
        for ty in &species.types {
            let mut candidates: Vec<_> = self
                .pool
                .iter()
                .copied()
                .filter(|id| stab_candidate(id, &|t| t == ty))
                .collect();
            while self.required_stab(ty) && !candidates.is_empty() {
                let mo = candidates.swap_remove(index(self.rng, candidates.len()));
                self.add(mo);
            }
        }
        if self.counts.type_count(state.preferred) == 0 {
            self.pick(|id| stab_candidate(id, &|t| t == state.preferred));
        }
        if !self.counts.stab {
            self.pick(|id| stab_candidate(id, &has_type));
        }
        if matches!(role, "Bulky Support" | "Bulky Attacker" | "Bulky Setup") {
            self.pick(|id| RECOVERY.contains(&id));
        }
        if role.contains("Setup")
            && !self.pick(|id| SETUP.contains(&id) && !SPEED_SETUP.contains(&id))
        {
            self.pick(|id| SETUP.contains(&id));
        }
        if state.doubles {
            for id in [
                "finalgambit",
                "mortalspin",
                "shedtail",
                "followme",
                "ragepowder",
            ] {
                self.force(id);
            }
            if role.contains("Protect") {
                self.pick(|id| PROTECT.contains(&id));
            }
            if role == "Doubles Support" {
                self.force("fakeout");
            }
            if role == "Doubles Wallbreaker" || species.base_stats.spe <= 50 {
                self.force("trickroom");
            }
            if matches!(role, "Doubles Fast Attacker" | "Doubles Support") {
                self.pick(|id| SPEED_CONTROL.contains(&id));
            }
            if role == "Doubles Fast Attacker" {
                if self.pool.contains(&"fakeout") {
                    self.force("fakeout");
                } else {
                    self.pick(|id| PROTECT.contains(&id));
                }
            }
            if role == "Doubles Bulky Setup" && !self.counts.setup {
                self.pick(|id| RECOVERY.contains(&id));
                self.pick(|id| PROTECT.contains(&id));
            }
        }
        if self.counts.damaging.is_empty() {
            self.pick(|id| {
                !NO_STAB.contains(&id) && set::move_ref(id).1.category != Category::Status
            });
        }
        if !matches!(
            role,
            "Fast Support"
                | "Bulky Support"
                | "Bulky Setup"
                | "Doubles Bulky Setup"
                | "Doubles Support"
        ) && let [attack] = self.counts.damaging.as_slice()
        {
            let current = &set::move_ref(attack).1.move_type;
            self.pick(|id| stab_candidate(id, &|t| t != current));
        }
        while self.moves.len() < state.max_moves && !self.pool.is_empty() {
            let id = sample(self.rng, &self.pool);
            self.add(id);
            for &(a, b) in PAIRS {
                if id == a {
                    self.force(b);
                }
                if id == b {
                    self.force(a);
                }
            }
        }
    }
}
