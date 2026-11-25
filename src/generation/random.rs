//! Team-level selection for the pinned Champions Random Battle builders.
//! Set construction is a separate stage and receives the same shared PRNG.
//! Based on Pokémon Showdown (MIT, see docs/LICENSE.pokemon-showdown).

use super::{GenerationError, index, sample};
use crate::{
    choice::to_id,
    dex::{DEX, Format, GameType, Species},
    input::Set,
    rng::Rng,
};
use serde::Deserialize;
use std::{collections::HashMap, sync::LazyLock};

#[derive(Deserialize)]
struct Catalog {
    revision: String,
    types: Vec<String>,
    formats: [Vec<Recipe>; 2],
}

#[derive(Deserialize)]
struct Recipe {
    id: String,
    mega: bool,
}

#[derive(Clone, Copy)]
struct Candidate {
    recipe: &'static Recipe,
    species: &'static Species,
}

struct Member {
    candidate: Candidate,
    set: Set,
}

static DATA: LazyLock<Catalog> = LazyLock::new(|| {
    let data: Catalog = serde_json::from_str(include_str!("../../data/random-champions.json"))
        .expect("checked-in Random Battle recipes must deserialize");
    assert_eq!(data.revision, DEX.revision);
    data
});

#[derive(Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
struct TeamDetails {
    rain: u8,
    sun: u8,
    sand: u8,
    snow: u8,
    status_cure: u8,
    spikes: u8,
    toxic_spikes: u8,
    stealth_rock: u8,
    sticky_web: u8,
    defog: u8,
    rapid_spin: u8,
    screens: u8,
}

impl TeamDetails {
    fn notice(&mut self, set: &Set) {
        let moves = |ids: &[&str]| set.moves.iter().any(|m| ids.contains(&m.as_str()));
        self.rain |= u8::from(set.ability == "Drizzle" || moves(&["raindance"]));
        self.sun |= u8::from(set.ability == "Drought" || moves(&["sunnyday"]));
        self.sand |= u8::from(set.ability == "Sand Stream");
        self.snow |=
            u8::from(set.ability == "Snow Warning" || moves(&["snowscape", "chillyreception"]));
        self.status_cure |= u8::from(moves(&["healbell"]));
        self.spikes += u8::from(moves(&["spikes", "ceaselessedge"]));
        self.toxic_spikes |= u8::from(set.ability == "Toxic Debris" || moves(&["toxicspikes"]));
        self.stealth_rock |= u8::from(moves(&["stealthrock", "stoneaxe"]));
        self.sticky_web |= u8::from(moves(&["stickyweb"]));
        self.defog |= u8::from(moves(&["defog"]));
        self.rapid_spin |= u8::from(moves(&["rapidspin", "mortalspin"]));
        self.screens |= u8::from(moves(&["auroraveil"]));
    }
}

fn assemble(
    format: &Format,
    rng: &mut Rng,
    mut set_for: impl FnMut(
        Candidate,
        &TeamDetails,
        bool,
        bool,
        &mut Rng,
    ) -> Result<Set, GenerationError>,
) -> Result<Vec<Member>, GenerationError> {
    let start_seed = rng.seed();
    let doubles = format.game_type == GameType::Doubles;
    let types: Vec<_> = DATA.types.iter().map(String::as_str).collect();
    // Even the standard (non-Monotype) builder samples a type first.
    rng.below(types.iter().filter(|&&ty| ty != "Stellar").count() as u32);
    let mut pool = Vec::<Vec<Candidate>>::new();
    let mut groups = HashMap::new();
    for recipe in &DATA.formats[usize::from(doubles)] {
        let species = DEX.species(format.dex_mod, &recipe.id).unwrap();
        let family = *groups
            .entry(species.base_species.as_str())
            .or_insert_with(|| {
                let i = pool.len();
                pool.push(Vec::new());
                i
            });
        pool[family].push(Candidate { recipe, species });
    }
    let mut base_pool = Vec::new();
    for (family, formes) in pool.iter().enumerate() {
        let weighted = if formes[0].species.base_species == "Squawkabilly" {
            1
        } else {
            formes.len().div_ceil(3).min(3)
        };
        base_pool.extend(std::iter::repeat_n(family, weighted));
    }
    let mut used = vec![false; pool.len()];
    let mut team = Vec::with_capacity(format.max_team_size);
    let mut type_count = HashMap::new();
    let mut weak = HashMap::new();
    let mut double_weak = HashMap::new();
    let mut details = TeamDetails::default();
    let mut has_mega = false;
    let limit = |n: usize| ((n * format.max_team_size + 3) / 6).max(1);

    while !base_pool.is_empty() && team.len() < format.max_team_size {
        let family = base_pool.swap_remove(index(rng, base_pool.len()));
        let formes = &pool[family];
        let choose_mega = !has_mega && formes.iter().any(|c| c.recipe.mega);
        let eligible: Vec<_> = formes
            .iter()
            .copied()
            .filter(|c| c.recipe.mega == choose_mega)
            .collect();
        if eligible.is_empty() {
            return Err(GenerationError(
                "cannot sample an empty Random Battle family".into(),
            ));
        }
        let candidate = sample(rng, &eligible);
        let species = candidate.species;
        if used[family] || (species.base_species == "Zoroark" && team.is_empty()) {
            continue;
        }
        if species
            .types
            .iter()
            .any(|ty| count(&type_count, ty) >= limit(2))
        {
            continue;
        }
        let weak_to_type = |ty| effectiveness(ty, species);
        if types.iter().any(|&ty| {
            let eff = weak_to_type(ty);
            (eff > 0 && count(&weak, ty) >= limit(3))
                || (eff > 1 && count(&double_weak, ty) >= limit(1))
        }) {
            continue;
        }
        let neutral_to_fire = weak_to_type("Fire") == 0;
        if neutral_to_fire
            && count(&weak, "Fire") >= limit(3)
            && species
                .abilities
                .values()
                .any(|s| matches!(s.as_str(), "Dry Skin" | "Fluffy"))
        {
            continue;
        }
        let ice = weak_to_type("Ice");
        let weak_to_freeze_dry =
            ice > 0 || (ice > -2 && species.types.iter().any(|ty| ty == "Water"));
        if (weak_to_freeze_dry && count(&weak, "Freeze-Dry") >= limit(4))
            || !compatible(&candidate.recipe.id, &team, doubles)
        {
            continue;
        }

        let lead = (team.len() + 1 == format.max_team_size
            || (doubles && team.len() + 2 == format.max_team_size))
            && format.picked_team_size.is_none()
            && !format.has_rule("teampreview");
        let set = set_for(candidate, &details, lead, doubles, rng)?;
        has_mega |= DEX
            .item(format.dex_mod, &to_id(&set.item))
            .is_some_and(|item| item.mega_stone.is_some());
        details.notice(&set);
        let member = Member { candidate, set };
        let never_lead: &[&str] = if doubles {
            &["basculegion", "basculegionf", "houndstone"]
        } else {
            &["kingambit"]
        };
        if lead && never_lead.contains(&candidate.recipe.id.as_str()) {
            team.push(member);
        } else {
            team.insert(0, member);
        }
        if team.len() == format.max_team_size {
            break;
        }
        used[family] = true;
        for ty in &species.types {
            increment(&mut type_count, ty);
        }
        for &ty in &types {
            let eff = weak_to_type(ty);
            if eff > 0 {
                increment(&mut weak, ty);
            }
            if eff > 1 {
                increment(&mut double_weak, ty);
            }
        }
        let set = if lead && never_lead.contains(&candidate.recipe.id.as_str()) {
            &team.last().unwrap().set
        } else {
            &team[0].set
        };
        if neutral_to_fire && matches!(set.ability.as_str(), "Dry Skin" | "Fluffy") {
            increment(&mut weak, "Fire");
        }
        if weak_to_freeze_dry {
            increment(&mut weak, "Freeze-Dry");
        }
    }
    if team.len() < format.max_team_size && team.len() < 12 {
        return Err(GenerationError(format!(
            "could not build a random team for {} (seed={start_seed})",
            format.name
        )));
    }
    Ok(team)
}

fn count(counts: &HashMap<&str, usize>, ty: &str) -> usize {
    counts.get(ty).copied().unwrap_or_default()
}

fn increment<'a>(counts: &mut HashMap<&'a str, usize>, ty: &'a str) {
    *counts.entry(ty).or_default() += 1;
}

fn effectiveness(attacking_type: &str, species: &Species) -> i8 {
    species
        .types
        .iter()
        .map(|ty| {
            match DEX
                .type_data(&to_id(ty))
                .unwrap()
                .damage_taken
                .get(attacking_type)
            {
                Some(1) => 1,
                Some(2) => -1,
                _ => 0, // Showdown's getEffectiveness does not check immunity.
            }
        })
        .sum()
}

fn compatible(id: &str, team: &[Member], doubles: bool) -> bool {
    const WEB: &[&str] = &["ariados", "slurpuff", "araquanid"];
    const SCREENS: &[&str] = &[
        "ninetalesalola",
        "abomasnow",
        "abomasnowmega",
        "froslassmega",
        "vanilluxe",
        "aurorus",
        "grimmsnarl",
        "meowstic",
    ];
    const SUN: &[&str] = &["charizardmegay", "ninetales", "torkoal"];
    const RAIN: &[&str] = &["politoed", "pelipper"];
    const SAND: &[&str] = &["tyranitar", "tyranitarmega", "hippowdon"];
    const SNOW: &[&str] = &[
        "ninetalesalola",
        "abomasnow",
        "abomasnowmega",
        "froslassmega",
        "vanilluxe",
        "aurorus",
    ];
    const LIGHTNING_ROD: &[&str] = &["pikachu", "raichu", "manectric"];
    let screens = |id| SCREENS.contains(&id) || (doubles && id == "klefki");
    let band = |id, groups: &[&[&str]]| groups.iter().position(|g| g.contains(&id));
    let weather = |id| band(id, &[SUN, RAIN, SAND, SNOW]);
    let terrain = |id| {
        band(
            id,
            &[
                &["pincurchin"],
                &["indeedee", "indeedeef"],
                &["rillaboom", "arboliva"],
            ],
        )
    };
    let conflict = |a: Option<usize>, b: Option<usize>| a.is_some() && b.is_some() && a != b;
    let dry_skin = |id| matches!(id, "toxicroak" | "heliolisk");
    team.iter().all(|p| {
        let other = p.candidate.recipe.id.as_str();
        !(WEB.contains(&id) && WEB.contains(&other)
            || (screens(id) && (screens(other) || other == "mrrime"))
            || (id == "mrrime" && screens(other))
            || (dry_skin(id) && SUN.contains(&other))
            || (dry_skin(other) && SUN.contains(&id))
            || (doubles
                && ((LIGHTNING_ROD.contains(&id) && LIGHTNING_ROD.contains(&other))
                    || conflict(weather(id), weather(other))
                    || conflict(terrain(id), terrain(other)))))
    })
}

#[derive(Deserialize)]
struct Fixture {
    revision: String,
    cases: Vec<FixtureCase>,
}

#[derive(Deserialize)]
struct FixtureCase {
    format: String,
    seed: String,
    after: String,
    sets: Vec<RecordedSet>,
    order: Vec<String>,
}

#[derive(Deserialize)]
struct RecordedSet {
    id: String,
    before: String,
    after: String,
    details: TeamDetails,
    lead: bool,
    set: Set,
}

#[test]
fn team_assembly_preserves_set_context_order_and_rng_across_random_formats() {
    let data: Fixture =
        serde_json::from_str(include_str!("../../tests/fixtures/random-assembly.json")).unwrap();
    assert_eq!(data.revision, DEX.revision);
    for case in data.cases {
        let format = DEX.format(&case.format).unwrap();
        let mut rng = Rng::new(case.seed.parse().unwrap());
        let mut recorded = case.sets.into_iter();
        let output = assemble(
            format,
            &mut rng,
            |candidate, details, lead, doubles, rng| {
                let expected = recorded.next().unwrap();
                let context = format!("{} seed {}", format.name, case.seed);
                assert_eq!(candidate.recipe.id, expected.id, "{context}");
                assert_eq!(rng.seed(), expected.before.parse().unwrap(), "{context}");
                assert_eq!(*details, expected.details, "{context}");
                assert_eq!(lead, expected.lead, "{context}");
                assert_eq!(doubles, format.game_type == GameType::Doubles, "{context}");
                // Replay only the not-yet-ported set builder's draws, in the test
                // harness. The production API never loads recorded sets.
                *rng = Rng::new(expected.after.parse().unwrap());
                Ok(expected.set)
            },
        )
        .unwrap();
        assert!(recorded.next().is_none());
        assert_eq!(
            output
                .iter()
                .map(|m| m.candidate.recipe.id.as_str())
                .collect::<Vec<_>>(),
            case.order
        );
        assert_eq!(rng.seed(), case.after.parse().unwrap());
    }
}
