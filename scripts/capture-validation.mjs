#!/usr/bin/env node
// Oracle verdicts for ordinary Champions sets and roster construction.
import assert from 'node:assert/strict';
import {readFileSync, writeFileSync} from 'node:fs';
import {createRequire} from 'node:module';

const stage = new URL('../.tools/poke-rl-simulator/', import.meta.url);
const require = createRequire(new URL('worker.cjs', stage));
const revision = '29f2d9969159fd23f8bb96e177a70072fe573b0e';
const lock = JSON.parse(readFileSync(new URL('package-lock.json', stage)));
assert(lock.packages['node_modules/pokemon-showdown'].resolved.endsWith('/' + revision));
const {TeamValidator} = require('pokemon-showdown');
const base = {species: 'Venusaur', moves: ['Sludge Bomb', 'Sleep Powder', 'Energy Ball', 'Protect'],
    ability: 'Chlorophyll', nature: 'Modest', evs: {hp: 32, spa: 32, spd: 2}};
const eternal = {species: 'Floette-Eternal', moves: ['Light of Ruin', 'Moonblast', 'Trick', 'Giga Drain'], ability: 'Flower Veil'};
const sets = [
    ['legal set', {}],
    ['unknown species', {species: 'Not a Pokémon'}],
    ['past item', {item: 'Ability Shield'}],
    ['mythical species', {species: 'Mew'}],
    ['unknown item', {item: 'Some fake item'}],
    ['unknown nature', {nature: 'Not a Nature'}],
    ['wrong ability', {ability: 'Adaptability'}],
    ['missing ability', {ability: ''}],
    ['empty moves', {moves: []}],
    ['duplicate move', {moves: ['Tackle', 'Tackle']}],
    ['unlearnable move', {moves: ['Dragon Ascent']}],
    ['unknown move', {moves: ['example'], ability: 'Adaptability'}],
    ['stat point bounds', {evs: {hp: 40, atk: -1, spa: 32, spd: 9}}],
    ['perfect IVs', {ivs: {atk: 0}}],
    ['level bounds', {level: 1000}],
    ['species nickname', {name: 'Bulbasaur'}],
    ['starting form of Mega', {species: 'Venusaur-Mega', item: 'Venusaurite', ability: 'Thick Fat'}],
    ['cosmetic forme inherits learnset', {species:'Furfrou-Heart', ability:'Fur Coat', moves:['Protect']}],
    ['modern event can use mints', {...eternal, level: 50, nature: 'Timid'}],
    ['shiny-locked event', {...eternal, level: 50, shiny: true}],
    ['underlevel event', {...eternal, level: 70}],
];
const corpus = JSON.parse(readFileSync(new URL('../benchmarks/verified-bss-corpus.json', import.meta.url))).teams;
const team = corpus[0].team;
const teams = [
    ['legal roster', team],
    ['too few members', team.slice(0, 5)],
    ['too many members', [...team, base]],
    ['duplicate species', [...team.slice(0, 5), {...team[0], item: ''}]],
    ['duplicate items', team.map((set, i) => i < 2 ? {...set, item: 'Leftovers'} : set)],
    ['duplicate nicknames', team.map((set, i) => i < 2 ? {...set, name: 'Twice'} : set)],
];
const formats = ['gen9championsbssregmc', 'gen9championsvgc2026regmc'];
const fixtures = formats.flatMap(format => {
    const validator = new TeamValidator(format);
    return [
        ...sets.map(([name, changes]) => ({name, format, set: {...base, ...changes},
            expected: validator.validateSet(structuredClone({...base, ...changes}))})),
        ...teams.map(([name, team]) => ({name, format, team,
            expected: validator.validateTeam(structuredClone(team))})),
    ];
});
const ids = {
    ou: 'gen9championsou', uu: 'gen9championsuu', draft: 'gen9championsdraft',
    doubles: 'gen9championsdoubles', duu: 'gen9champions4v4doublesuu',
    doublesDraft: 'gen9champions4v4doublesdraft',
    natdex: 'gen9championsnatdexdraft',
};
const simple = (species, ability, moves, item = '') => ({species, ability, moves, item, nature: 'Hardy'});
const leagueTeam = [
    simple('Vaporeon', 'Water Absorb', ['Surf', 'Protect']),
    simple('Snorlax', 'Thick Fat', ['Body Slam', 'Protect']),
    simple('Umbreon', 'Synchronize', ['Foul Play', 'Protect']),
    simple('Arcanine', 'Intimidate', ['Flare Blitz', 'Protect']),
    simple('Clefable', 'Magic Guard', ['Gravity', 'Moonblast']),
    simple('Gengar', 'Cursed Body', ['Shadow Ball', 'Sludge Bomb']),
];
const leagueCases = [
    ['OU forbids direct sleep', ids.ou, 'set', base],
    ['OU Uber tier', ids.ou, 'set', simple('Gholdengo','Good as Gold',['Make It Rain','Protect'])],
    ['OU item ban', ids.ou, 'set', {...leagueTeam[0], item:'Damp Rock'}],
    ['UU inherits OU ban source', ids.uu, 'set', {...leagueTeam[0], item:'Damp Rock'}],
    ['UU OU tier', ids.uu, 'set', simple('Garchomp','Rough Skin',['Earthquake','Protect'])],
    ['UU direct ability ban', ids.uu, 'set', simple('Ninetales','Drought',['Flamethrower','Protect'])],
    ['Doubles DUber tier', ids.doubles, 'set', simple('Archaludon','Stamina',['Flash Cannon','Protect'])],
    ['4v4 Doubles UU bans all forms of a base species', ids.duu, 'set', simple('Garchomp','Rough Skin',['Earthquake','Protect'])],
    ['4v4 Doubles UU checks the held Mega Stone forme', ids.duu, 'set', simple('Charizard','Blaze',['Flamethrower'],'Charizardite Y')],
    ['Draft permits Sleep Powder', ids.draft, 'set', base],
    ['Doubles Draft uses Flat Rules', ids.doublesDraft, 'set', base],
    ['NatDex accepts old transfer TM', ids.natdex, 'set', simple('Charizard','Blaze',['Flamethrower','Toxic','Roost'])],
    ['NatDex accepts Return and typed Hidden Power', ids.natdex, 'set', simple('Pidgeot','Keen Eye',['Brave Bird','Return','Hidden Power Ice'])],
    ['NatDex uses Champions fallback for an exclusive move', ids.natdex, 'set', eternal],
    ['NatDex checks Sketch instead of the ordinary learnset', ids.natdex, 'set', simple('Smeargle','Own Tempo',['Spore','Sticky Web','Celebrate','Protect'])],
    ['NatDex validates moves from a past event', ids.natdex, 'set', simple('Pikachu','Static',['Volt Tackle','Surf'],'Light Ball')],
    ['NatDex admits an event-only Pokémon when its origin matches', ids.natdex, 'set', simple('Victini','Victory Star',['V-create'])],
    ['NatDex retains an obtainable past item', ids.natdex, 'set', simple('Pikachu','Static',['Thunderbolt'],'Normal Gem')],
    ['NatDex declines an item never obtainable in Gen 7 to 9', ids.natdex, 'set', simple('Pikachu','Static',['Thunderbolt'],'Electric Gem')],
    ['NatDex permits a past species and move', ids.natdex, 'set', simple('Castform','Forecast',['Weather Ball','Sunny Day','Hail'])],
    ['NatDex declines an impossible form', ids.natdex, 'set', simple('Eternatus-Eternamax','Pressure',['Recover'])],
    ['NatDex retains the CAP ban', ids.natdex, 'set', simple('Kitsunoh','Limber',['Shadow Strike'])],
    ['NatDex declines a move without a valid source', ids.natdex, 'set', simple('Pidgeot','Keen Eye',['Brave Bird','Dragon Ascent'])],
    ['NatDex checks typed Hidden Power against the explicit type', ids.natdex, 'set',
        {...simple('Charizard','Blaze',['Hidden Power Ice']), hpType:'Fairy'}],
    ...[
        ['plate determines Arceus forme', simple('Arceus','Multitype',['Judgment'],'Pixie Plate')],
        ['Arceus-Fairy held item mismatch', simple('Arceus-Fairy','Multitype',['Judgment'],'Leftovers')],
        ['drive determines Genesect forme', simple('Genesect','Download',['Techno Blast'],'Burn Drive')],
        ['memory determines Silvally forme', simple('Silvally','RKS System',['Multi-Attack'],'Water Memory')],
        ['core permits Levitate on Giratina', simple('Giratina','Levitate',['Shadow Force'],'Griseous Core')],
        ['orb is not the required item for Giratina-Origin', simple('Giratina-Origin','Levitate',['Shadow Force'],'Griseous Orb')],
        ['crystal chooses Dialga-Origin', simple('Dialga','Pressure',['Roar of Time'],'Adamant Crystal')],
        ['mask permits forme-specific Ogerpon ability', simple('Ogerpon','Water Absorb',['Ivy Cudgel'],'Wellspring Mask')],
        ['borrowed move determines Ultra Necrozma starting forme', simple('Necrozma-Ultra','Neuroforce',['Sunsteel Strike'],'Ultranecrozium Z')],
        ['Moongeist Beam chooses Dawn Wings', simple('Necrozma-Ultra','Neuroforce',['Moongeist Beam'],'Ultranecrozium Z')],
        ['Ultra Necrozma needs its crystal', simple('Necrozma-Ultra','Neuroforce',['Moongeist Beam'],'Life Orb')],
        ['Ultra Necrozma cannot infer either of two forms', simple('Necrozma-Ultra','Neuroforce',['Protect'],'Ultranecrozium Z')],
        ['Ultra Necrozma cannot infer a form from both signatures', simple('Necrozma-Ultra','Neuroforce',['Moongeist Beam','Sunsteel Strike'],'Ultranecrozium Z')],
        ['Zygarde-Complete requires an explicit starting form', simple('Zygarde-Complete','Power Construct',['Thousand Arrows'])],
        ['Zygardite needs Power Construct', simple('Zygarde','Aura Break',['Thousand Arrows'],'Zygardite')],
        ['Power Construct in the 10 percent form', simple('Zygarde-10%','Power Construct',['Thousand Arrows'])],
        ['Terapagos changes forme with Tera Shift', simple('Terapagos-Stellar','Teraform Zero',['Tera Starstorm'])],
        ['Battle Bond uses the Greninja-Bond event', simple('Greninja','Battle Bond',['Water Shuriken'])],
        ['Greninja-Bond event excludes shiny', {...simple('Greninja','Battle Bond',['Water Shuriken']), gender:'F', shiny:true}],
        ['Rockruff with Own Tempo is legal', simple('Rockruff','Own Tempo',['Rock Throw'])],
        ['Rockruff-Dusk rejects other abilities', simple('Rockruff-Dusk','Vital Spirit',['Rock Throw'])],
        ['Secret Sword requires Resolute Keldeo', simple('Keldeo','Justified',['Secret Sword'])],
        ['Keldeo needs Secret Sword to stay Resolute', simple('Keldeo-Resolute','Justified',['Protect'])],
        ['Hoopa-Unbound cannot keep Hyperspace Hole', simple('Hoopa-Unbound','Magician',['Hyperspace Hole'])],
        ['Hoopa cannot keep Hyperspace Fury', simple('Hoopa','Magician',['Hyperspace Fury'])],
        ['Relic Song is needed for Meloetta-Pirouette', simple('Meloetta-Pirouette','Serene Grace',['Psychic'])],
        ['Behemoth Blade is legalized through Iron Head on Zacian-Crowned', simple('Zacian-Crowned','Intrepid Sword',['Behemoth Blade'],'Rusted Sword')],
        ['Behemoth Bash is legalized through Iron Head on Zamazenta-Crowned', simple('Zamazenta-Crowned','Dauntless Shield',['Behemoth Bash'],'Rusted Shield')],
        ['holding a Red Orb permits the Primal ability', simple('Groudon','Desolate Land',['Precipice Blades'],'Red Orb')],
        ['holding a Blue Orb permits the Primal ability', simple('Kyogre','Primordial Sea',['Origin Pulse'],'Blue Orb')],
        ['Dragon Ascent permits Delta Stream in NatDex', simple('Rayquaza','Delta Stream',['Dragon Ascent'])],
        ['Rayquaza must know Dragon Ascent to Mega Evolve', simple('Rayquaza-Mega','Delta Stream',['Protect'])],
        ['Darkrai learns its signature sleep move', simple('Darkrai','Bad Dreams',['Dark Void'])],
        ['Smeargle cannot Sketch Dark Void', simple('Smeargle','Own Tempo',['Dark Void'])],
    ].map(([name, set]) => [name, ids.natdex, 'set', set]),
];
for (const format of Object.values(ids)) {
    leagueCases.push(['ordinary full roster', format, 'team', leagueTeam]);
}
for (const format of [ids.doubles, ids.duu, ids.doublesDraft]) {
    leagueCases.push(['Gravity with accuracy-limited sleep on a separate teammate', format, 'team',
        [...leagueTeam.slice(0, 5), {...leagueTeam[5], moves:['Hypnosis','Shadow Ball']}]]);
}
for (const format of [ids.ou, ids.draft]) {
    leagueCases.push(['duplicate items and nicknames follow format clauses', format, 'team', leagueTeam.map((s,i) =>
        ({...s, ...(i<2 && {name:'Twice'}), ...(i<4 && {item:i===0||i===3?'Leftovers':'Sitrus Berry'})}))]);
}
for (const [name, format, key, value] of leagueCases) {
    const validator = new TeamValidator(format);
    const method = key === 'set' ? 'validateSet' : 'validateTeam';
    fixtures.push({name, format, [key]: value, expected: validator[method](structuredClone(value))});
}
writeFileSync(new URL('../tests/fixtures/validation.json', import.meta.url),
    JSON.stringify({revision, fixtures}, null, 2) + '\n');
