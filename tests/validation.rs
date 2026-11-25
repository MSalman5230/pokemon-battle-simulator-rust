use pokemon_battle_simulator::{
    dex::{DEX, Mod},
    validation::{LEGALITY, MoveSources},
};

#[test]
fn champions_legality_uses_the_pinned_learnsets_and_regulation_table() {
    assert_eq!(LEGALITY.revision, DEX.revision);
    let bss = LEGALITY.format("gen9championsbssregmc").unwrap();
    assert_eq!(bss.ev_limit, Some(66));
    assert_eq!(bss.item_limit, 1);
    assert_eq!(bss.rule_sources.get("-tag:mythical").unwrap(), "Flat Rules");
    assert_eq!(
        LEGALITY.species(Mod::Champions, "venusaur").unwrap().tier,
        "RU"
    );
    assert_eq!(LEGALITY.species(Mod::RegMb, "venusaur").unwrap().tier, "UU");
    assert_eq!(
        LEGALITY.move_sources_by_name(Mod::Champions, "Venusaur", "Energy Ball", 50),
        MoveSources::Unrestricted
    );
    assert_eq!(
        LEGALITY.move_sources_by_name(Mod::Champions, "Venusaur-Mega", "Energy Ball", 50),
        MoveSources::Unrestricted
    );
    assert_eq!(
        LEGALITY.move_sources_by_name(Mod::Champions, "Venusaur", "Dragon Ascent", 50),
        MoveSources::Absent
    );
}
