#!/usr/bin/env node
// Rebuild fixtures with the exact Showdown dependency used by Poke-RL.
import assert from 'node:assert/strict';
import {readFileSync, writeFileSync} from 'node:fs';
import {createRequire} from 'node:module';

const stage = new URL('../.tools/poke-rl-simulator/', import.meta.url);
const require = createRequire(new URL('worker.cjs', stage));
const {Battle, BattleStream, PRNG, TeamValidator, Teams} = require('pokemon-showdown');
const {extractChannelMessages} = require('pokemon-showdown/dist/sim/battle');
const revision = '29f2d9969159fd23f8bb96e177a70072fe573b0e';
const lock = JSON.parse(readFileSync(new URL('package-lock.json', stage)));
assert(lock.packages['node_modules/pokemon-showdown'].resolved.endsWith('/' + revision));
const save = (file, data) => writeFileSync(new URL('../tests/fixtures/' + file, import.meta.url),
    JSON.stringify({revision, ...data}, null, 2) + '\n');

const seeds = ['0,0,0,0', '1,2,3,4', '65535,65535,65535,65535', 'gen5,ffff0a09abcdffff',
    'sodium,a', 'sodium,131415161718191a1b1c1d1e1f202122',
    'sodium,ffffff00000000000000000a0000000000000000000000000000000000000000'];
const rng = seeds.map(seed => {
    const prng = new PRNG(seed);
    const words = Array.from({length: 64}, () => prng.rng.next());
    const bounded = [0, 1, 24, 16, 100, 2, 65536, 4294967295].map(upper => [upper, prng.random(upper)]);
    return {seed, words, bounded, after: prng.getSeed()};
});

const fallback = [90, 10, 10, 70, 40, 90, 40, 10];
const sorting = [fallback, Array(20).fill(50), fallback.concat(Array(18).fill(300))].map((speeds, n) => {
    const seed = seeds[n];
    const battle = new Battle({formatid: 'gen9championsbssregmc', seed});
    const nodes = speeds.map((speed, i) => ({speed, i, priority: [0, -0.1, 0.1][(i + n) % 3]}));
    if (n > 0) nodes.forEach(p => p.priority = 0);
    const input = structuredClone(nodes);
    battle.speedSort(nodes);
    return {seed, input, output: nodes.map(x => x.i), after: battle.prng.getSeed()};
});

const battles = ['bssregmc', 'bssregmb', 'randombattle', 'customgame']
    .map(name => new Battle({formatid: 'gen9champions' + name, seed: seeds[1]}));
const stat = battles.flatMap((battle, mode) => ['atk', 'spe', 'hp'].flatMap(name =>
    [0, 1, 2, 32, 255].map((points, i) => {
        const base = [50, 100, 95, 250, 255][i];
        const level = [1, 42, 50, 100, 9999][i];
        const nature = ['AdaMant', 'timid', 'serious', 'brave', 'hasty'][i];
        const set = {evs: {[name]: points}, level, nature};
        return {mode, name, points, base, level, nature, result: battle.statModify({[name]: base}, set, name)};
    })));
const normal = battles[0];
const healthMon = {species: 'Magikarp', moves: ['Splash'], gender: 'F'};
normal.setPlayer('p1', {team: [healthMon]});
const mon = normal.sides[0].pokemon[0];
const health = [
    [0, 201, ''], [1, 333, ''], [40, 200, ''], [41, 204, ''], [100, 200, ''],
    [100, 199, 'brn'], [200, 200, 'tox'], [51, 103, ''],
].flatMap(([hp, maxhp, status]) => [false, true].map(exact => {
    Object.assign(mon, {hp, maxhp, status});
    normal.reportExactHP = exact;
    const {secret, shared} = mon.getHealth();
    return {hp, maxhp, status, exact, secret, shared};
}));
save('mechanics.json', {rng, sorting, stat, health});

const named = (species, moves, gender = 'F') => ({
    species, moves, gender, name: species, level: 50, nature: 'Hardy', ability: '',
});
const rosters = [
    [named('Flareon', ['Tackle', 'Quick Attack', 'Protect']), named('Vaporeon', ['Water Gun', 'Surf']),
        named('Jolteon', ['Thunder Shock']), named('Glaceon', ['Ice Shard'])],
    [named('Umbreon', ['Tackle']), named('Espeon', ['Swift', 'Protect']), named('Leafeon', ['Tackle', 'Protect']),
        named('Sylveon', ['Quick Attack'])],
];
const replacementRosters = [
    [{...named('Vaporeon', ['Surf']), evs: {spa: 32}, nature: 'Modest'}, named('Jolteon', ['Protect']),
        named('Glaceon', ['Ice Shard'])],
    [named('Camerupt', ['Tackle']), named('Rhydon', ['Tackle']), named('Flareon', ['Quick Attack'])],
];
const competitiveSets = [
    ['Infernape', 'Blaze', ['Close Combat', 'Protect', 'Swords Dance', 'Earthquake']],
    ['Empoleon', 'Torrent', ['Surf', 'Aqua Jet', 'Swords Dance', 'Protect']],
    ['Meganium', 'Overgrow', ['Swords Dance', 'Earthquake', 'Leaf Blade', 'Protect']],
    ['Swampert', 'Torrent', ['Earthquake', 'Surf', 'Protect', 'Flip Turn']],
    ['Sceptile', 'Overgrow', ['Leaf Blade', 'Earthquake', 'Swords Dance', 'Protect']],
    ['Blaziken', 'Blaze', ['Close Combat', 'Protect', 'Swords Dance', 'Earthquake']],
].map(([species, ability, moves]) => ({
    ...named(species, moves), ability, nature: 'Adamant', evs: {hp: 2, atk: 32, spe: 32},
}));
const competitiveRosters = [competitiveSets, [...competitiveSets].reverse()];
const statusRosters = [
    [named('Vaporeon', ['Thunder Wave', 'Recover', 'Surf', 'Ice Beam']),
        named('Flareon', ['Will-O-Wisp', 'Tackle']), named('Jolteon', ['Nuzzle'])],
    [named('Umbreon', ['Toxic', 'Tackle', 'Protect', 'Hypnosis']),
        {...named('Glaceon', ['Tackle', 'Ice Beam', 'Recover', 'Protect']), item: 'Leftovers'}, named('Espeon', ['Swift'])],
];
const sleepFreezeRosters = [
    [named('Jolteon', ['Ice Beam', 'Recover']), named('Flareon', ['Flamethrower'])],
    [named('Umbreon', ['Tackle', 'Hypnosis', 'Recover']), named('Vaporeon', ['Recover'])],
];
const itemRosters = [
    [{...named('Flareon', ['Tackle']), item: 'Life Orb'},
        {...named('Vaporeon', ['Surf']), item: 'Mystic Water'},
        {...named('Jolteon', ['Earthquake', 'Water Gun', 'Toxic', 'Recover']), item: 'Sitrus Berry'}],
    [{...named('Garchomp', ['Protect', 'Tackle']), ability: 'Rough Skin', item: 'Rocky Helmet'},
        {...named('Camerupt', ['Protect', 'Tackle']), item: 'Focus Sash'},
        {...named('Gholdengo', ['Shadow Ball', 'Recover']), ability: 'Good as Gold', item: 'Air Balloon'}],
];
const weatherRosters = [
    [{...named('Pelipper', ['Rain Dance', 'Protect']), ability: 'Drizzle', item: 'Damp Rock'},
        {...named('Floatzel', ['Water Gun', 'Protect', 'Rain Dance']), ability: 'Swift Swim'},
        {...named('Ninetales', ['Sunny Day', 'Water Gun', 'Protect']), ability: 'Drought'},
        named('Vaporeon', ['Water Gun', 'Recover'])],
    [{...named('Tyranitar', ['Sandstorm', 'Protect', 'Water Gun']), ability: 'Sand Stream'},
        {...named('Excadrill', ['Tackle', 'Protect', 'Rain Dance']), ability: 'Sand Rush'},
        {...named('Glaceon', ['Snowscape', 'Ice Beam', 'Protect']), ability: 'Slush Rush'},
        named('Umbreon', ['Tackle'])],
];
const hazardRosters = [
    [named('Smeargle', ['Spikes', 'Stealth Rock', 'Toxic Spikes', 'Sticky Web']),
        named('Vaporeon', ['Recover']), named('Flareon', ['Tackle'])],
    [named('Umbreon', ['Tackle', 'Recover']), named('Glaceon', ['Recover']),
        {...named('Gengar', ['Recover']), item: 'Heavy-Duty Boots'}],
];
const pivotRosters = [
    [named('Flareon', ['U-turn', 'Quick Attack']), named('Vaporeon', ['Flip Turn', 'Surf']),
        named('Jolteon', ['Volt Switch']), named('Glaceon', ['Ice Shard'])],
    rosters[1],
];
const megaRosters = [
    [{...named('Charizard', ['Flamethrower', 'Protect']), ability: 'Blaze', item: 'Charizardite Y'},
        {...named('Garchomp', ['Tackle', 'Earthquake']), ability: 'Rough Skin', item: 'Garchompite Z'},
        {...named('Vaporeon', ['Surf', 'Protect']), ability: 'Torrent'}, named('Glaceon', ['Snowscape'])],
    [{...named('Tyranitar', ['Rock Slide', 'Protect', 'Earthquake']), ability: 'Sand Stream', item: 'Tyranitarite'},
        {...named('Umbreon', ['Tackle']), item: 'Leftovers'}, named('Espeon', ['Swift']), named('Leafeon', ['Tackle'])],
];
const trappedRosters = [
    [named('Swampert', ['Tackle', 'Protect']), named('Starmie', ['Surf']), named('Umbreon', ['Tackle'])],
    [{...named('Dugtrio', ['Tackle']), ability: 'Arena Trap'}, named('Leafeon', ['Leaf Blade']),
        named('Espeon', ['Protect'])],
];
const mbRosters = [
    [{...named('Glaceon', ['Tackle', 'Wish']), shiny: true}, named('Flareon', ['Tackle']),
        named('Vaporeon', ['Surf']), named('Zacian-Crowned', ['Tackle'])],
    rosters[1],
];
const imprisonRosters = [
    [named('Espeon', ['Imprison', 'Tackle']), named('Flareon', ['Tackle'])],
    [named('Vaporeon', ['Tackle', 'Water Gun']), named('Umbreon', ['Recover'])],
];
const struggleRosters = [
    [named('Vaporeon', ['Recover']), named('Flareon', ['Tackle'])],
    [named('Glaceon', ['Protect']), named('Umbreon', ['Recover'])],
];
const choiceRosters = [
    [{...named('Garchomp', ['Tackle', 'Knock Off', 'Dragon Claw']), ability: 'Rough Skin', item: 'Choice Scarf'},
        {...named('Blaziken', ['Protect', 'Close Combat']), ability: 'Speed Boost'}, named('Vaporeon', ['Surf'])],
    [{...named('Espeon', ['Swift', 'Protect']), item: 'Choice Specs'},
        {...named('Incineroar', ['Knock Off', 'Protect']), ability: 'Intimidate', item: 'Leftovers'},
        {...named('Dragonite', ['Tackle']), ability: 'Multiscale'}],
];
const bulkyRosters = [
    [{...named('Corviknight', ['Iron Defense', 'Roost', 'Body Press', 'Brave Bird']), ability: 'Mirror Armor'},
        named('Vaporeon', ['Surf']), named('Umbreon', ['Tackle'])],
    [{...named('Archaludon', ['Icy Wind', 'Body Press', 'Giga Drain', 'Protect']), ability: 'Stamina'},
        named('Glaceon', ['Freeze-Dry']), named('Jolteon', ['Tackle'])],
];
const screenRosters = [
    [{...named('Ninetales-Alola', ['Aurora Veil', 'Reflect', 'Tackle', 'Rest']), ability: 'Snow Warning', item: 'Light Clay'},
        named('Pelipper', ['Surf']), named('Umbreon', ['Recover'])],
    [{...named('Vaporeon', ['Water Gun', 'Yawn', 'Recover', 'Tackle']), ability: 'Water Absorb'},
        named('Glaceon', ['Freeze-Dry']), named('Jolteon', ['Tackle'])],
];
const cuteCharmRosters = [
    [named('Espeon', ['Tackle', 'Recover'], 'M'), named('Flareon', ['Tackle'])],
    [{...named('Sylveon', ['Tackle', 'Recover']), ability: 'Cute Charm'}, named('Umbreon', ['Tackle'])],
];
const cursedBodyRosters = [
    [named('Jolteon', ['Water Gun', 'Recover']), named('Flareon', ['Tackle'])],
    [{...named('Gengar', ['Recover', 'Disable']), ability: 'Cursed Body'}, named('Umbreon', ['Tackle'])],
];
const typeRosters = [
    [{...named('Meowscarada', ['Flower Trick', 'Thunderbolt', 'Tackle']), ability: 'Protean'},
        {...named('Machamp', ['Fire Fang']), ability: 'No Guard'}, named('Glaceon', ['Tackle'])],
    [{...named('Sylveon', ['Hyper Voice', 'Tackle', 'Recover']), ability: 'Pixilate'},
        named('Vaporeon', ['Recover']), named('Umbreon', ['Tackle'])],
];
const redirectRosters = [
    [named('Jolteon', ['Thunderbolt', 'Water Gun', 'Surf']),
        {...named('Raichu', ['Protect', 'Quick Attack']), ability: 'Lightning Rod'},
        {...named('Gastrodon', ['Recover']), ability: 'Storm Drain'}],
    [named('Umbreon', ['Recover', 'Thunder Wave']),
        {...named('Rhydon', ['Recover']), ability: 'Lightning Rod'},
        {...named('Vaporeon', ['Water Gun']), ability: 'Water Absorb'}],
];
const magicGuardRosters = [
    [{...named('Clefable', ['Double-Edge', 'Recover']), ability: 'Magic Guard', item: 'Life Orb'},
        named('Vaporeon', ['Recover'])],
    [{...named('Garchomp', ['Stealth Rock', 'Toxic', 'Sandstorm', 'Water Gun']),
        ability: 'Rough Skin', item: 'Rocky Helmet'}, named('Umbreon', ['Recover'])],
];
const lastResortRosters = [
    [named('Snorlax', ['Tackle', 'Water Gun', 'Last Resort']), named('Vaporeon', ['Recover'])],
    [named('Umbreon', ['Recover']), named('Glaceon', ['Recover'])],
];
const armorRosters = [
    [{...named('Metagross', ['Tackle', 'Meteor Mash', 'Water Gun']), ability: 'Clear Body', item: 'Expert Belt'},
        {...named('Arcanine', ['Water Gun', 'Toxic', 'Recover']), ability: 'Intimidate', item: 'Wise Glasses'}],
    [{...named('Armarouge', ['Recover']), ability: 'Weak Armor'},
        {...named('Torkoal', ['Recover', 'Growl']), ability: 'White Smoke'},
        {...named('Dragonair', ['Recover']), ability: 'Shed Skin'}],
];
const rechargeRosters = [
    [{...named('Slaking', ['Hyper Beam', 'Tackle']), ability: 'No Guard'}, named('Vaporeon', ['Recover'])],
    [named('Umbreon', ['Protect', 'Thunder Wave', 'Recover']), named('Glaceon', ['Recover'])],
];
const weatherAttackRosters = [
    [{...named('Pelipper', ['Hurricane', 'Weather Ball', 'Thunder', 'Sunny Day']), ability: 'Drizzle'},
        {...named('Wigglytuff', ['Blizzard', 'Weather Ball']), ability: 'Pixilate'}],
    [named('Goodra', ['Recover', 'Snowscape', 'Rain Dance']), named('Vaporeon', ['Recover'])],
];
const rampageRosters = [
    [named('Shuckle', ['Tackle', 'Outrage']), named('Vaporeon', ['Recover'])],
    [{...named('Umbreon', ['Recover', 'Protect']), ability: 'Pressure'}, named('Jolteon', ['Recover'])],
];
const tauntRosters = [
    [{...named('Weavile', ['Taunt', 'Fake Out', 'Tackle']), ability: 'Inner Focus'}, named('Vaporeon', ['Recover'])],
    [named('Snorlax', ['Recover', 'Tackle', 'Protect']),
        {...named('Slowbro', ['Recover', 'Water Gun']), ability: 'Oblivious'}],
];
const tradeEffectsRosters = [
    [{...named('Shuckle', ['Court Change', 'Glaive Rush', 'Recover']), item: 'Leftovers'}, named('Jolteon', ['Recover'])],
    [named('Dusknoir', ['Recover', 'Stealth Rock', 'Poltergeist', 'Reflect']), named('Vaporeon', ['Recover'])],
];
const copiedAbilityRosters = [
    [{...named('Metagross', ['Meteor Mash', 'Water Gun']), ability: 'Clear Body'},
        {...named('Porygon2', ['Water Gun', 'Recover']), ability: 'Trace'}],
    [{...named('Cofagrigus', ['Recover']), ability: 'Mummy'},
        {...named('Vaporeon', ['Water Gun', 'Recover']), ability: 'Water Absorb'}],
];
const hpEffectRosters = [
    [named('Vileplume', ['Growth', 'Strength Sap', 'Pain Split', 'Sunny Day']), named('Umbreon', ['Recover'])],
    [named('Crobat', ['Tackle', 'Swords Dance']), named('Shuckle', ['Recover'])],
];
const multiHitRosters = [
    [named('Weavile', ['Triple Axel', 'Recover']),
        {...named('Cloyster', ['Icicle Spear']), ability: 'Skill Link'},
        {...named('Armaldo', ['Rock Blast']), item: 'Loaded Dice'}],
    [{...named('Mudsdale', ['Recover']), ability: 'Stamina'},
        {...named('Umbreon', ['Recover']), item: 'Sitrus Berry'}],
];
const sashMultiHitRosters = [
    [{...named('Cloyster', ['Icicle Spear']), ability: 'Skill Link', evs: {atk: 32}, nature: 'Adamant'},
        named('Vaporeon', ['Recover'])],
    [{...named('Noibat', ['Recover']), item: 'Focus Sash'}, named('Jolteon', ['Recover']), named('Umbreon', ['Recover'])],
];
const trickRoomRosters = [
    [named('Bronzor', ['Trick Room', 'Tackle', 'Recover']), named('Umbreon', ['Recover', 'Quick Attack'])],
    [named('Jolteon', ['Recover', 'Tackle']), named('Vaporeon', ['Recover', 'Tackle'])],
];
const grassyRosters = [
    [{...named('Rillaboom', ['Grassy Glide', 'Grassy Terrain', 'Recover']), ability: 'Grassy Surge', item: 'Terrain Extender'},
        {...named('Flygon', ['Earthquake', 'Recover']), ability: 'Levitate'}],
    [named('Glaceon', ['Tackle', 'Ice Spinner', 'Recover']), named('Umbreon', ['Recover', 'Quick Attack'])],
];
const otherTerrainRosters = [
    [named('Slowbro', ['Electric Terrain', 'Misty Terrain', 'Psychic Terrain', 'Recover']), named('Vaporeon', ['Recover'])],
    [named('Umbreon', ['Yawn', 'Thunder Wave', 'Quick Attack', 'Tackle']), named('Jolteon', ['Recover'])],
];
const encoreRosters = [
    [named('Weavile', ['Encore', 'Recover']), named('Jolteon', ['Recover'])],
    [named('Umbreon', ['Quick Attack', 'Recover']), named('Glaceon', ['Recover'])],
];
const mentalHerbRosters = [encoreRosters[0], [{...encoreRosters[1][0], item: 'Mental Herb'}, encoreRosters[1][1]]];
const faintedTargetRosters = [
    [named('Shedinja', ['Tackle']), named('Vaporeon', ['Recover'])],
    [{...named('Snorlax', ['Stealth Rock']), item: 'Rocky Helmet'}, named('Umbreon', ['Recover'])],
];
const tiedDisableRosters = [
    [{...named('Garchomp', ['Tackle', 'Recover']), item: 'Choice Band'}, named('Vaporeon', ['Recover'])],
    [named('Umbreon', ['Taunt', 'Water Gun']), named('Snorlax', ['Recover'])],
];
const stackingMovesRosters = [
    [named('Maushold', ['Tidy Up', 'Stored Power', 'Supercell Slam', 'Recover']),
        {...named('Clefable', ['Supercell Slam']), ability: 'Magic Guard'}],
    [named('Golem', ['Spikes', 'Stealth Rock', 'Water Gun']), named('Snorlax', ['Recover'])],
];
const ownTempoRosters = [
    [{...named('Slowbro', ['Rain Dance', 'Recover']), ability: 'Own Tempo'}, named('Vaporeon', ['Recover'])],
    [{...named('Arcanine', ['Hurricane', 'Recover']), ability: 'Intimidate'}, named('Umbreon', ['Recover'])],
];
const phazingRosters = [
    [{...named('Druddigon', ['Dragon Tail']), ability: 'No Guard'}, named('Vaporeon', ['Recover'])],
    [{...named('Gastrodon', ['Recover']), ability: 'Suction Cups'},
        {...named('Umbreon', ['Recover']), ability: 'Pressure'}, named('Glaceon', ['Recover'])],
];
const jointPhazingRosters = [
    [{...named('Weavile', ['Dragon Tail']), ability: 'No Guard'}, named('Vaporeon', ['Recover'])],
    [named('Slowbro', ['Dragon Tail']), named('Umbreon', ['Recover']), named('Glaceon', ['Recover'])],
];
const retaliationRosters = [
    [named('Meowscarada', ['Tackle', 'Recover', 'U-turn']), named('Vaporeon', ['Seismic Toss', 'Recover'])],
    [named('Slowbro', ['Metal Burst', 'Recover', 'Super Fang']), named('Umbreon', ['Recover'])],
];
const attentionRosters = [
    [named('Weavile', ['Thunderbolt']), {...named('Raichu', ['Thunderbolt', 'Recover']), ability: 'Lightning Rod'},
        named('Meowscarada', ['Thunderbolt'])],
    [named('Umbreon', ['Follow Me', 'Rage Powder', 'Recover']), named('Vaporeon', ['Recover'])],
];
const conditionalSecondaryRosters = [
    [named('Wigglytuff', ['Alluring Voice', 'Follow Me', 'Dire Claw', 'Recover']), named('Vaporeon', ['Recover'])],
    [named('Umbreon', ['Calm Mind', 'Recover']), named('Glaceon', ['Recover'])],
];
const leechRosters = [
    [{...named('Tangrowth', ['Leech Seed', 'Recover']), ability: 'No Guard'}, named('Swampert', ['Recover'])],
    [named('Umbreon', ['Tackle']), {...named('Clefable', ['Tackle']), ability: 'Magic Guard'}, named('Vileplume', ['Tackle'])],
];
const magicianRosters = [
    [{...named('Delphox', ['Water Gun']), ability: 'Magician'}, named('Umbreon', ['Recover'])],
    [{...named('Vaporeon', ['Tackle']), item: 'Leftovers'}, named('Glaceon', ['Recover'])],
];
const symbiosisRosters = [
    [{...named('Grimmsnarl', ['Recover']), item: 'Sitrus Berry'},
        {...named('Florges', ['Recover']), ability: 'Symbiosis', item: 'Leftovers'}],
    [named('Hariyama', ['Close Combat']), named('Vaporeon', ['Recover'])],
];
const gluttonyRosters = [
    [named('Heracross', ['Drain Punch']), named('Umbreon', ['Recover'])],
    [{...named('Snorlax', ['Tackle']), ability: 'Gluttony', item: 'Salac Berry'}, named('Vaporeon', ['Recover'])],
];
const ohkoRosters = [
    [{...named('Dugtrio', ['Fissure', 'Sheer Cold']), ability: 'No Guard'}, named('Vaporeon', ['Recover'])],
    [{...named('Geodude', ['Recover']), ability: 'Sturdy'}, named('Umbreon', ['Recover']), named('Glaceon', ['Recover'])],
];
const destinyBondRosters = [
    [named('Gengar', ['Destiny Bond', 'Tackle']), named('Vaporeon', ['Recover'])],
    [{...named('Slowking-Galar', ['Recover', 'Psychic']), evs: {spa: 32}, nature: 'Modest'}, named('Torkoal', ['Recover'])],
];
const trickRosters = [
    [{...named('Rotom-Wash', ['Trick', 'Foul Play']), ability: 'Levitate', item: 'Choice Scarf'}, named('Vaporeon', ['Recover'])],
    [{...named('Snorlax', ['Water Gun', 'Recover']), item: 'Leftovers'}, {...named('Slowbro', ['Recover']), ability: 'Sticky Hold'}],
];
const wanderingRosters = [
    [{...named('Heracross', ['Aerial Ace', 'Skill Swap']), ability: 'Guts'}, named('Vaporeon', ['Recover'])],
    [{...named('Runerigus', ['Recover']), ability: 'Wandering Spirit'}, named('Umbreon', ['Recover'])],
];
const chargeRosters = [
    [named('Archaludon', ['Recover', 'Rain Dance', 'Electro Shot']),
        {...named('Meganium', ['Solar Beam', 'Sunny Day', 'Recover']), item: 'Power Herb'}],
    [named('Umbreon', ['Recover']), named('Goodra', ['Recover'])],
];
const psychicSpreadRosters = [
    [{...named('Indeedee-F', ['Expanding Force']), ability: 'Psychic Surge'}, named('Flareon', ['Recover'])],
    [named('Vaporeon', ['Recover']), named('Glaceon', ['Protect', 'Recover'])],
];
const stakeoutRosters = [
    [{...named('Mabosstiff', ['Tackle', 'Water Gun']), ability: 'Stakeout'}, named('Flareon', ['Recover'])],
    [named('Umbreon', ['Recover']), named('Vaporeon', ['Recover'])],
];
const diveRosters = [
    [named('Vaporeon', ['Dive', 'Recover']), named('Umbreon', ['Recover'])],
    [named('Slowbro', ['Surf', 'Tackle', 'Recover']), {...named('Machamp', ['Tackle']), ability: 'No Guard'}],
];
const resistBerryRosters = [
    [{...named('Haxorus', ['Close Combat']), ability: 'Unnerve'},
        {...named('Heracross', ['Tackle', 'Close Combat']), item: 'Life Orb'}],
    [{...named('Snorlax', ['Recover']), item: 'Chople Berry'},
        {...named('Glaceon', ['Recover']), item: 'Chilan Berry'}],
];
const flowerVeilRosters = [
    [{...named('Florges', ['Recover']), ability: 'Flower Veil'},
        {...named('Rillaboom', ['Bulk Up']), ability: 'Overgrow'}, named('Vaporeon', ['Recover'])],
    [{...named('Arcanine', ['Recover']), ability: 'Intimidate'},
        named('Espeon', ['Charm', 'Will-O-Wisp', 'Yawn', 'Icy Wind'])],
];
const partingShotRosters = [
    [named('Jolteon', ['Parting Shot']), named('Vaporeon', ['Recover'])],
    [{...named('Diancie', ['Recover']), ability: 'Clear Body'},
        {...named('Corviknight', ['Recover']), ability: 'Mirror Armor'}, named('Snorlax', ['Recover'])],
];
const moldBreakerRosters = [
    [{...named('Haxorus', ['Earthquake', 'Dragon Claw', 'Will-O-Wisp', 'Skill Swap']), ability: 'Mold Breaker'},
        named('Jolteon', ['Recover'])],
    [{...named('Rotom-Wash', ['Recover']), ability: 'Levitate', evs: {hp: 32, def: 32}, nature: 'Bold'},
        {...named('Garchomp', ['Recover']), ability: 'Rough Skin', evs: {hp: 32, def: 32}},
        {...named('Mismagius', ['Recover']), ability: 'Levitate', item: 'Ability Shield'}],
];
const breakerRedirectRosters = [
    [{...named('Haxorus', ['Thunderbolt', 'Will-O-Wisp']), ability: 'Mold Breaker'},
        named('Jolteon', ['Thunderbolt', 'Will-O-Wisp'])],
    [{...named('Rhydon', ['Recover']), ability: 'Lightning Rod'}, named('Vaporeon', ['Recover']),
        {...named('Gholdengo', ['Recover']), ability: 'Good as Gold'}],
];
const illusionRosters = [
    [{...named('Zoroark-Hisui', ['Nasty Plot', 'Shadow Ball']), name: 'disguiser', ability: 'Illusion', item: 'Rocky Helmet'},
        {...named('Vaporeon', ['Recover']), name: 'reserve'}, {...named('Espeon', ['Recover']), name: 'decoy', shiny: true}],
    [named('Umbreon', ['Will-O-Wisp', 'Peck', 'Recover']), named('Glaceon', ['Recover'])],
];
const illusionPartnerRosters = [[illusionRosters[0][0], illusionRosters[0][2]], illusionRosters[1]];
const whiteHerbRosters = [
    [{...named('Vaporeon', ['Work Up', 'Close Combat', 'Recover']), item: 'White Herb'},
        {...named('Glaceon', ['Recover']), item: 'White Herb'}],
    [named('Snorlax', ['Recover']), {...named('Arcanine', ['Recover']), ability: 'Intimidate'}],
];
const conditionalPriorityRosters = [
    [{...named('Honchkrow', ['Sucker Punch', 'Thunderclap']), ability: 'Moxie'}, named('Glaceon', ['Recover'])],
    [{...named('Weavile', ['Tackle', 'Quick Attack', 'Recover']), ability: 'Inner Focus'}, named('Snorlax', ['Tackle'])],
];
const weightRosters = [
    [{...named('Incineroar', ['Grass Knot', 'Low Kick', 'Heavy Slam', 'Heat Crash']), ability: 'Intimidate'}, named('Glaceon', ['Recover'])],
    [{...named('Snorlax', ['Recover']), ability: 'Thick Fat'},
        {...named('Umbreon', ['Recover']), ability: 'Light Metal', item: 'Float Stone'},
        {...named('Aggron', ['Recover']), ability: 'Heavy Metal'}],
];
const scaleShotRosters = [
    [{...named('Haxorus', ['Scale Shot']), ability: 'Mold Breaker', item: 'Loaded Dice'}, named('Glaceon', ['Recover'])],
    [{...named('Corviknight', ['Recover']), ability: 'Pressure'}, {...named('Sylveon', ['Protect']), ability: 'Cute Charm'}],
];
const endeavorRosters = [
    [{...named('Eevee', ['Endeavor']), ability: 'Adaptability'}, named('Glaceon', ['Recover'])],
    [named('Snorlax', ['Amnesia', 'Tackle']), named('Mismagius', ['Recover'])],
];
const cloudNineRosters = [
    [{...named('Pelipper', ['Weather Ball', 'Water Gun', 'Rain Dance', 'Recover']), ability: 'Drizzle'}, named('Glaceon', ['Recover'])],
    [{...named('Altaria', ['Recover']), ability: 'Cloud Nine'},
        {...named('Arcanine', ['Sandstorm', 'Recover']), ability: 'Intimidate'}],
];
const hpPowerRosters = [
    [{...named('Typhlosion', ['Eruption']), ability: 'Blaze'}, named('Vaporeon', ['Water Spout'])],
    [{...named('Snorlax', ['Quick Attack']), ability: 'Thick Fat'}, named('Pelipper', ['Protect'])],
];
const stanceRosters = [
    [{...named('Aegislash', ['Shadow Ball', "King's Shield", 'Recover'], 'M'), ability: 'Stance Change'}, named('Vaporeon', ['Recover'])],
    [named('Umbreon', ['Bite', 'Toxic', 'Recover']), named('Glaceon', ['Recover'])],
];
const bounceRosters = [
    [{...named('Espeon', ['Toxic', 'Stealth Rock', 'Recover']), ability: 'Magic Bounce'}, named('Glaceon', ['Recover'])],
    [{...named('Hatterene', ['Yawn', 'Recover']), ability: 'Magic Bounce'}, named('Vaporeon', ['Recover'])],
];
const bounceAllyRosters = [
    [named('Glaceon', ['Spikes', 'Recover']), named('Vaporeon', ['Recover'])],
    [named('Umbreon', ['Recover']), bounceRosters[1][0]],
];
const bouncePivotRosters = [
    [named('Jolteon', ['Parting Shot', 'Recover']), named('Glaceon', ['Recover'])],
    [{...bounceRosters[1][0], moves: ['Tackle']}, named('Vaporeon', ['Recover'])],
];
const digRosters = [
    [{...named('Eevee', ['Dig']), ability: 'Adaptability', nature: 'Bold', evs: {hp: 32, def: 32}}, named('Glaceon', ['Recover'])],
    [named('Snorlax', ['Tackle', 'Earthquake', 'Recover', 'Sandstorm']), named('Vaporeon', ['Recover'])],
];
for (const [suffix, play, players = rosters, label = suffix, seed = seeds[1]] of [
    ['bssregmc', [['team 132', 'team 231'], ['move 2', 'move 1'], ['switch 2', 'move 1']]],
    ['vgc2026regmc', [['team 1234', 'team 2134'], ['move 1 2, move 2', 'move 2, move 1 1'],
        ['move 2 2, switch 4', 'move 1, move 1 1']]],
    ['vgc2026regmc', [['team 123', 'team 123'], ['move 1, move 1', 'move 1 1, move 1 1'],
        [null, 'pass, switch 3'], ['move 1, move 1', 'move 1 1'],
        ['move 1, move 1', 'move 1 1']], replacementRosters, 'vgc-replacements'],
    ['bssregmc', [['team 123', 'team 365'], ['move 3', 'move 2'], ['move 1', 'move 3'],
        ['move 1', 'move 2'], [null, 'switch 2'], ['move 1', 'move 2']],
        competitiveRosters, 'bss-competitive'],
    ['bssregmc', [['team 123', 'team 123'], ['move 1', 'move 1'], ['move 2', 'move 2'],
        ['switch 2', 'move 2'], ['move 1', 'switch 2'], ['move 2', 'move 1'],
        ['switch 2', 'move 3'], ['move 4', 'move 2'], ['move 2', 'move 2']],
        statusRosters, 'bss-status'],
    ['bssregmc', [['team 12', 'team 12'], ['move 1', 'move 2'],
        ...Array(3).fill(['move 2', 'move 2']), ...Array(3).fill(['move 2', 'move 1'])],
        sleepFreezeRosters, 'bss-sleep-freeze', '1,2,3,22'],
    ['bssregmc', [['team 123', 'team 123'], ['move 1', 'move 2'],
        ['switch 2', 'switch 2'], ['move 1', 'move 2'], ['move 1', 'move 2'],
        [null, 'switch 3'], ['switch 3', 'move 2'], ['move 1', 'move 1'],
        ['move 3', 'move 2'], ['move 2', 'move 2'], ['move 4', 'move 1']], itemRosters, 'bss-items'],
    ['vgc2026regmc', [['team 1234', 'team 1234'], ['move 2, move 1 1', 'move 1, move 1 2'],
        ['switch 3, move 1 2', 'move 3 2, move 1 2'],
        ['move 3, move 3', 'move 2, move 1 2'], ['move 2 1, move 1 2', 'move 2, switch 3'],
        ['move 2 1, move 1 2', 'move 2, move 1']], weatherRosters, 'vgc-weather'],
    ['bssregmc', [['team 123', 'team 123'], ['move 1', 'move 2'], ['move 2', 'move 2'],
        ['move 3', 'move 2'], ['move 3', 'move 2'], ['move 4', 'move 2'],
        ['move 1', 'switch 2'], ['move 4', 'switch 3'], ['move 2', 'switch 2']],
        hazardRosters, 'bss-hazards'],
    ['vgc2026regmc', [['team 1234', 'team 2134'], ['move 1 2, move 2', 'move 2, move 1 2'],
        ['switch 3', null], ['move 1 2, move 1 1', 'switch 3, move 1 1'],
        ['switch 3', null], ['pass, switch 4', null]], pivotRosters, 'vgc-pivots'],
    ['vgc2026regmc', [['team 1234', 'team 1234'], ['move 2 mega, move 1 2', 'move 2 mega, move 1 1'],
        ['switch 3, move 1 2', 'move 2, move 1 1'], ['switch 3, move 1 2', 'move 2, move 1 1']],
        megaRosters, 'vgc-megas'],
    ['bssregmc', [['team 213', 'team 123'], ['move 1 mega', 'move 3'], ['move 2', 'move 2']],
        megaRosters, 'bss-mega-z'],
    ['bssregmc', [['team 123', 'team 365'], ['move 3', 'move 2']], competitiveRosters, 'bss-packed'],
    ['bssregmb', [['team 123', 'team 123'], ['move 1', 'move 1']], mbRosters, 'bss-reg-mb'],
    ['bssregmc', [['team 123', 'team 123'], ['switch 2', 'move 1'], ['move 1', null]], trappedRosters, 'bss-hidden-trap'],
    ['bssregmc', [['team 12', 'team 12'], ['move 1', 'move 1'], ['move 2', 'move 1'],
        [null, 'move 2'], ['switch 2', 'move 2']], imprisonRosters, 'bss-imprison'],
    ['bssregmc', [['team 12', 'team 12'], ...Array(10).fill(['move 1', 'move 1'])],
        struggleRosters, 'bss-struggle'],
    ['bssregmc', [['team 123', 'team 123'], ['move 1', 'move 1'], ['switch 2', 'switch 2'],
        ['move 1', 'move 1'], ['switch 2', 'move 1'], ['move 2', 'switch 3']],
        choiceRosters, 'bss-choice-items'],
    ['bssregmc', [['team 123', 'team 123'], ['move 1', 'move 1'], ['move 2', 'move 2'],
        ['move 3', 'move 3'], ['move 4', 'switch 2'], ['switch 2', 'move 1']],
        bulkyRosters, 'bss-bulky-effects'],
    ['bssregmc', [['team 123', 'team 123'], ['move 1', 'move 4'], ['move 2', 'move 2'],
        ['move 3', 'move 1'], ['move 3', 'move 1'], ['move 3', 'move 1'],
        ['move 4', 'move 4'], ['switch 2', 'move 4'], ['move 1', 'move 4'], ['move 1', 'move 4']],
        screenRosters, 'bss-screen-rest'],
    ['bssregmc', [['team 12', 'team 12'], ['move 1', 'move 1'], ['move 1', 'move 1'],
        ['move 1', 'switch 2']], cuteCharmRosters, 'bss-cute-charm', '1,2,3,3'],
    ['bssregmc', [['team 12', 'team 12'], ['move 1', 'move 1'], ['move 2', 'move 2'],
        ['move 2', 'move 1'], ['move 2', 'move 1'], ['move 1', 'move 1']],
        cursedBodyRosters, 'bss-cursed-body', '1,2,3,3'],
    ['bssregmc', [['team 123', 'team 123'], ['move 1', 'move 1'], ['move 2', 'move 3'],
        ['switch 2', 'move 3'], ['move 1', 'move 2'], ['switch 2', 'move 3'], ['move 3', 'move 1']],
        typeRosters, 'bss-converted-types'],
    ['doublescustomgame', [['team 1234', 'team 2134'], ['move 1 2, move 2', 'move 2, move 1 1']]],
    ['ou', [['team 1324', 'team 2314'], ['move 2', 'move 1']]],
    ['vgc2026regmc', [['team 123', 'team 123'], ['move 1 1, move 2 2', 'move 1, move 1'],
        ['move 2 1, switch 3', 'move 2 1, switch 3'], ['move 3, move 1', 'move 1, move 1 1']],
        redirectRosters, 'vgc-elemental-redirection'],
    ['bssregmc', [['team 12', 'team 12'], ['move 2', 'move 1'], ['move 1', 'move 2'],
        ['switch 2', 'move 3'], ['switch 2', 'move 4'], ['move 1', 'move 4']],
        magicGuardRosters, 'bss-magic-guard'],
    ['bssregmc', [['team 12', 'team 12'], ['move 1', 'move 1'], ['move 2', 'move 1'],
        ['move 3', 'move 1'], ['switch 2', 'move 1'], ['switch 2', 'move 1']],
        lastResortRosters, 'bss-last-resort'],
    ['bssregmc', [['team 12', 'team 123'], ['move 1', 'move 1'], ['move 3', 'move 1'],
        ['move 2', 'switch 2'], ['move 2', 'move 2'], ['switch 2', 'move 1'], ['move 1', 'move 1'],
        ['move 2', 'switch 3'], ['move 2', 'move 1'], ['move 3', 'move 1'], ['move 3', 'move 1']],
        armorRosters, 'bss-damage-hooks'],
    ['bssregmc', [['team 12', 'team 12'], ['move 1', 'move 1'], ['move 1', 'move 2'],
        ['move recharge', 'move 3'], ['move 2', 'move 3']], rechargeRosters, 'bss-recharge'],
    ['vgc2026regmc', [['team 12', 'team 12'], ['move 1 1, move 1', 'move 2 1, move 1'],
        ['move 1, move 1', 'move 3, move 1']], rechargeRosters, 'vgc-recharge'],
    ['bssregmc', [['team 12', 'team 12'], ['move 1', 'move 1'], ['move 2', 'move 1'],
        ['move 3', 'move 1'], ['move 4', 'move 1'], ['move 1', 'move 1'],
        ['switch 2', 'move 2'], ['move 1', 'move 1'], ['move 2', 'move 1']],
        weatherAttackRosters, 'bss-weather-attacks', '1,2,3,6'],
    ['bssregmc', [['team 12', 'team 12'], ['move 2', 'move 1'], ['move 1', 'move 1'],
        ['move 1', 'move 1'], ['move 1', 'move 1']], rampageRosters, 'bss-rampage', '1,2,3,1'],
    ['vgc2026regmc', [['team 12', 'team 12'], ['move 2, move 1', 'move 1, move 1'],
        ['move 1, move 1', 'move 1, move 1'], ['move 1, move 1', 'move 1, move 1']],
        rampageRosters, 'vgc-rampage', '1,2,3,1'],
    ['bssregmc', [['team 12', 'team 12'], ['move 2', 'move 1'], ['move 1', 'move 1'],
        ['move 3', 'move 2'], ['move 3', 'move 2'], ['move 3', 'move 1'], ['move 1', 'switch 2']],
        tauntRosters, 'bss-taunt'],
    ['bssregmc', [['team 12', 'team 12'], ['move 1', 'move 1'], ['move 2', 'move 2'],
        ['move 1', 'move 3'], ['switch 2', 'move 3'], ['move 1', 'move 4'], ['switch 2', 'move 1'],
        ['move 1', 'move 1']], tradeEffectsRosters, 'bss-court-change'],
    ['bssregmc', [['team 12', 'team 12'], ['move 1', 'move 1'], ['move 2', 'switch 2'],
        ['switch 2', 'move 1'], ['move 1', 'move 1'], ['switch 2', 'move 2']],
        copiedAbilityRosters, 'bss-ability-transfer'],
    ['bssregmc', [['team 12', 'team 12'], ['move 1', 'move 1'], ['move 2', 'move 1'],
        ['move 4', 'move 2'], ['move 1', 'move 1'], ['move 3', 'move 1'], ['move 3', 'switch 2']],
        hpEffectRosters, 'bss-hp-effects'],
    ['bssregmc', [['team 123', 'team 12'], ['move 1', 'move 1'], ['switch 2', 'move 1'],
        ['move 1', 'move 1'], ['switch 3', 'switch 2'], ['move 1', 'move 1'], ['move 1', 'move 1']],
        multiHitRosters, 'bss-multihit', '1,2,3,1'],
    ['vgc2026regmc', [['team 12', 'team 123'], ['move 1 1, move 1', 'move 1, move 1'],
        [null, 'switch 3, pass']], sashMultiHitRosters, 'vgc-multihit-sash'],
    ['vgc2026regmc', [['team 12', 'team 12'], ['move 1, move 1', 'move 1, move 1'],
        ['move 2 1, move 2 2', 'move 2 1, move 1'], ['move 3, move 1', 'move 1, move 1'],
        ['move 3, move 1', 'move 1, move 1'], ['move 3, move 1', 'move 1, move 1'],
        ['move 1, move 1', 'move 1, move 1'], ['move 2 1, move 1', 'move 2 1, move 1'],
        ['move 1, move 1', 'move 1, move 1'], ['move 2 1, move 2 2', 'move 2 1, move 2 1']],
        trickRoomRosters, 'vgc-trick-room'],
    ['vgc2026regmc', [['team 12', 'team 12'], ['move 1 2, move 1', 'move 1 1, move 1'],
        ['move 1 2, move 2', 'move 2 1, move 1'], ['move 2, move 2', 'move 3, move 1']],
        grassyRosters, 'vgc-grassy-terrain'],
    ['bssregmc', [['team 12', 'team 12'], ['move 1', 'move 1'], ['move 4', 'move 4'],
        ['move 2', 'move 1'], ['move 3', 'move 2'], ['move 4', 'move 3']],
        otherTerrainRosters, 'bss-terrain-protection'],
    ['vgc2026regmc', [['team 12', 'team 12'], ['move 2, move 1', 'move 1 1, move 1'],
        ['move 1 1, move 1', 'move 2, move 1'], ['move 2, move 1', 'move 1 1, move 1'],
        ['move 2, move 1', 'move 1 1, move 1'], ['move 2, move 1', 'move 2, move 1']],
        encoreRosters, 'vgc-encore'],
    ['bssregmc', [['team 12', 'team 12'], ['move 2', 'move 1'], ['move 1', 'move 2'], ['move 2', 'move 1']],
        mentalHerbRosters, 'bss-mental-herb'],
    ['bssregmc', [['team 12', 'team 12'], ['move 1', 'move 1'], ['switch 2', null]],
        faintedTargetRosters, 'bss-hazard-after-faint'],
    ['bssregmc', [['team 12', 'team 12'], ['move 1', 'move 1'], ['move 1', 'move 2']],
        tiedDisableRosters, 'bss-tied-disables'],
    ['bssregmc', [['team 12', 'team 12'], ['move 2', 'move 2'], ['move 1', 'move 1'],
        ['move 1', 'move 3'], ['move 2', 'move 3'], ['move 3', 'move 3'], ['switch 2', 'move 3'], ['move 1', 'move 3']],
        stackingMovesRosters, 'bss-stacking-moves'],
    ['bssregmc', [['team 12', 'team 12'], ['move 1', 'move 2'], ['move 2', 'move 1'], ['move 2', 'move 1']],
        ownTempoRosters, 'bss-own-tempo', '1,2,3,6'],
    ['bssregmc', [['team 12', 'team 123'], ['move 1', 'move 1'], ['move 1', 'switch 2'], ['move 1', 'move 1']],
        phazingRosters, 'bss-phazing'],
    ['vgc2026regmc', [['team 12', 'team 123'], ['move 1 1, move 1', 'move 1 1, move 1']],
        jointPhazingRosters, 'vgc-phazing'],
    ['bssregmc', [['team 12', 'team 12'], ['move 1', 'move 1'], ['move 2', 'move 1'], ['move 3', 'move 1'],
        ['switch 2', null], ['move 1', 'move 3'], ['move 1', 'move 2']],
        retaliationRosters, 'bss-retaliation'],
    ['vgc2026regmc', [['team 123', 'team 12'], ['move 1 2, move 2', 'move 1, move 1'],
        ['switch 3, move 1 2', 'move 2, move 1'], ['move 1 2, move 2', 'move 2, move 1']],
        attentionRosters, 'vgc-attention'],
    ['bssregmc', [['team 12', 'team 12'], ['move 1', 'move 1'], ['move 2', 'move 2'],
        ['move 3', 'move 2'], ['move 3', 'move 2'], ['move 3', 'move 2'], ['move 4', 'move 2']],
        conditionalSecondaryRosters, 'bss-conditional-secondary', '1,2,3,6'],
    ['bssregmc', [['team 12', 'team 123'], ['move 1', 'move 1'], ['switch 2', 'move 1'],
        ['switch 2', 'switch 2'], ['move 1', 'move 1'], ['move 1', 'switch 3']], leechRosters, 'bss-leech-seed'],
    ['bssregmc', [['team 12', 'team 12'], ['move 1', 'move 1'], ['move 1', 'move 1']], magicianRosters, 'bss-magician'],
    ['vgc2026regmc', [['team 12', 'team 12'], ['move 1, move 1', 'move 1 1, move 1']],
        symbiosisRosters, 'vgc-symbiosis'],
    ['bssregmc', [['team 12', 'team 12'], ['move 1', 'move 1']], gluttonyRosters, 'bss-gluttony'],
    ['bssregmc', [['team 12', 'team 123'], ['move 1', 'move 1'], ['move 1', 'switch 2'], [null, 'switch 3'],
        ['move 2', 'move 1']], ohkoRosters, 'bss-ohko'],
    ['bssregmc', [['team 12', 'team 12'], ['move 1', 'move 1'], ['move 1', 'move 1'], ['move 1', 'move 2'],
        ['switch 2', 'switch 2']], destinyBondRosters, 'bss-destiny-bond'],
    ['bssregmc', [['team 12', 'team 12'], ['move 1', 'move 1'], ['move 2', 'move 1'], ['move 1', 'switch 2']], trickRosters, 'bss-trick-foul-play'],
    ['bssregmc', [['team 12', 'team 12'], ['move 1', 'move 1'], ['move 2', 'move 1'], ['move 1', 'move 1'],
        ['switch 2', 'move 1'], ['switch 2', 'move 1']], wanderingRosters, 'bss-wandering-spirit'],
    ['bssregmc', [['team 12', 'team 12'], ['move 3', 'move 1'], ['move 1', 'switch 2'], ['move 2', 'move 1'],
        ['move 3', 'move 1'], ['switch 2', 'move 1'], ['move 1', 'move 1'], ['move 2', 'move 1'], ['move 1', 'move 1']],
        chargeRosters, 'bss-charge'],
    ['vgc2026regmc', [['team 12', 'team 12'], ['move 3 2, move 3', 'move 1, move 1'],
        ['move 1, move 3', 'move 1, move 1']], chargeRosters, 'vgc-charge'],
    ['vgc2026regmc', [['team 12', 'team 12'], ['move 1 2, move 1', 'move 1, move 1'],
        ['move 1 1, move 1', 'move 1, move 2']], psychicSpreadRosters, 'vgc-expanding-force'],
    ['bssregmc', [['team 12', 'team 12'], ['move 1', 'move 1'], ['move 1', 'switch 2'],
        ['move 2', 'switch 2'], ['move 2', 'move 1']], stakeoutRosters, 'bss-stakeout'],
    ['bssregmc', [['team 12', 'team 12'], ['move 1', 'move 1'], ['move 1', 'move 3'], ['move 1', 'move 2'],
        ['move 1', 'move 3'], ['move 1', 'switch 2'], ['move 1', 'move 1'], ['move 1', 'move 1']], diveRosters, 'bss-dive'],
    ['bssregmc', [['team 12', 'team 12'], ['move 1', 'move 1'], ['switch 2', 'move 1'],
        ['move 1', 'move 1'], ['move 2', 'move 1'], ['move 1', 'switch 2'], ['move 1', 'move 1']],
        resistBerryRosters, 'bss-resist-berries'],
    ['vgc2026regmc', [['team 123', 'team 12'], ['move 1, move 1', 'move 1, move 1 2'],
        ['move 1, move 1', 'move 1, move 2 2'], ['move 1, move 1', 'move 1, move 3 2'],
        ['move 1, move 1', 'move 1, move 4'], ['switch 3, move 1', 'move 1, move 2 2']],
        flowerVeilRosters, 'vgc-flower-veil'],
    ['bssregmc', [['team 12', 'team 123'], ['move 1', 'move 1'], ['move 1', 'switch 2'],
        ['switch 2', null], ['switch 2', 'switch 3'], ['move 1', 'move 1'], ['switch 2', null]],
        partingShotRosters, 'bss-parting-shot'],
    ['bssregmc', [['team 12', 'team 123'], ['move 1', 'move 1'], ['move 2', 'switch 2'],
        ['move 1', 'switch 3'], ['move 4', 'move 1']], moldBreakerRosters, 'bss-mold-breaker'],
    ['vgc2026regmc', [['team 12', 'team 123'], ['move 1 2, move 1 2', 'move 1, move 1'],
        ['move 2 1, move 2 1', 'switch 3, move 1']], breakerRedirectRosters, 'vgc-mold-breaker'],
    ['bssregmc', [['team 123', 'team 12'], ['move 1', 'move 1'], ['move 2', 'move 2'],
        ['switch 3', 'move 3'], ['switch 3', 'move 3'], ['move 1', 'move 2']], illusionRosters, 'bss-illusion'],
    ['vgc2026regmc', [['team 12', 'team 12'], ['move 1, move 1', 'move 2 1, move 1']],
        illusionPartnerRosters, 'vgc-illusion'],
    ['bssregmc', [['team 12', 'team 12'], ['move 1', 'move 1'], ['move 2', 'move 1'],
        ['switch 2', 'switch 2'], ['move 1', 'move 1']], whiteHerbRosters, 'bss-white-herb'],
    ['bssregmc', [['team 12', 'team 12'], ['move 1', 'move 3'], ['move 1', 'move 1'],
        ['move 1', 'move 2'], ['move 1', 'switch 2'], ['move 2', 'move 1']],
        conditionalPriorityRosters, 'bss-conditional-priority'],
    ['bssregmc', [['team 12', 'team 123'], ['move 1', 'move 1'], ['move 2', 'move 1'],
        ['move 1', 'switch 2'], ['move 3', 'move 1'], ['move 4', 'switch 3']],
        weightRosters, 'bss-weight-moves'],
    ['bssregmc', [['team 12', 'team 12'], ['move 1', 'move 1'], ['move 1', 'switch 2'], ['move 1', 'move 1']],
        scaleShotRosters, 'bss-scale-shot'],
    ['bssregmc', [['team 12', 'team 12'], ['move 1', 'move 1'], ['move 1', 'move 2'], ['move 1', 'move 1'],
        ['move 1', 'switch 2']], endeavorRosters, 'bss-endeavor'],
    ['bssregmc', [['team 12', 'team 12'], ['move 1', 'move 1'], ['move 3', 'move 1'],
        ['move 2', 'switch 2'], ['move 4', 'move 1'], ['move 1', 'switch 2'], ['move 4', 'switch 2']],
        cloudNineRosters, 'bss-cloud-nine'],
    ['vgc2026regmc', [['team 12', 'team 12'], ['move 1, move 1', 'move 1 1, move 1']],
        hpPowerRosters, 'vgc-hp-power'],
    ['bssregmc', [['team 12', 'team 12'], ['move 1', 'move 1'], ['move 2', 'move 1'],
        ['move 1', 'move 3'], ['move 2', 'move 2'], ['move 1', 'move 3'], ['switch 2', 'move 3'],
        ['switch 2', 'move 3']], stanceRosters, 'bss-stance-change'],
    ['bssregmc', [['team 12', 'team 12'], ['move 1', 'move 2'], ['move 2', 'move 1'],
        ['move 1', 'move 2'], ['switch 2', 'move 2']], bounceRosters, 'bss-magic-bounce'],
    ['vgc2026regmc', [['team 12', 'team 12'], ['move 1, move 1', 'move 1, move 2']],
        bounceAllyRosters, 'vgc-magic-bounce'],
    ['bssregmc', [['team 12', 'team 12'], ['move 1', 'move 1'], [null, 'switch 2'], ['move 2', 'move 1']],
        bouncePivotRosters, 'bss-magic-bounce-pivot'],
    ['bssregmc', [['team 12', 'team 12'], ['move 1', 'move 1'], ['move 1', 'move 3'],
        ['move 1', 'move 2'], ['move 1', 'move 3'], ['move 1', 'move 4']], digRosters, 'bss-dig'],
]) {
    const formatid = 'gen9champions' + suffix;
    if (players === competitiveRosters) {
        const validator = new TeamValidator(formatid);
        for (const team of players) assert.equal(validator.validateTeam(structuredClone(team)), null);
        writeFileSync(new URL('../benchmarks/verified-bss-corpus.json', import.meta.url),
            JSON.stringify({revision, teams: players.map(team => ({team}))}, null, 2) + '\n');
    }
    const inputs = [
        '>start ' + JSON.stringify({formatid, seed, strictChoices: !["bss-hidden-trap", "bss-imprison"].includes(label)}),
        ...players.map((team, i) => '>player p' + (i + 1) + ' ' + JSON.stringify({name: 'player' + i, team: label === 'bss-packed' ?
            Teams.pack(team).replace('|Blaze|', '|0|').replace('|Torrent|', '||') : team})),
    ];
    const stream = new BattleStream({noCatch: true});
    let logCursor = 0;
    const checkpoints = [];
    async function checkpoint(lines) {
        for (const line of lines) await stream.write(line);
        const battle = stream.battle;
        const newLog = battle.log.slice(logCursor).filter(l => !l.startsWith('|t:|'));
        logCursor = battle.log.length;
        // Open Team Sheet prompts are UI, outside the headless engine.
        const channels = extractChannelMessages(newLog.join('\n'), [0, 1, 2]);
        checkpoints.push({
            lines,
            ...(label === 'bss-hidden-trap' && checkpoints.length === 2 && {unavailable: ['>p1 switch 2']}),
            ...(label === 'bss-imprison' && checkpoints.length === 3 && {unavailable: ['>p2 move 1']}),
            turn: battle.turn,
            seed: battle.prng.getSeed(),
            requestState: battle.requestState,
            waiting: battle.sides.map(s => s.isChoiceDone()),
            channels: [0, 1, 2].map(id => channels[id].filter(l => !l.startsWith('|uhtml|otsrequest|') && !l.startsWith('|uhtmlchange|otsrequest|'))),
            requests: battle.sides.map(s => structuredClone(s.activeRequest)),
            ...(['bss-last-resort', 'bss-taunt'].includes(label) && {observedDisables: battle.sides.map(s =>
                s.pokemon.map(p => p.moveSlots.map(m => !!m.disabled)))}),
            teams: battle.sides.map(s => s.pokemon.map(p => ({
                name: p.name, species: p.species.name, details: p.details, hp: p.hp, maxhp: p.maxhp, status: p.status,
                stats: {...p.storedStats}, boosts: {...p.boosts}, ability: p.ability, item: p.item, active: p.isActive,
                moves: p.moveSlots.map(m => ({id: m.id, pp: m.pp, maxpp: m.maxpp})),
            }))),
        });
    }
    await checkpoint(inputs);
    for (const choices of play) await checkpoint(choices.flatMap((c, i) => c ? ['>p' + (i + 1) + ' ' + c] : []));
    await stream.writeEnd();
    save(label + '.json', {formatid, checkpoints});
}
