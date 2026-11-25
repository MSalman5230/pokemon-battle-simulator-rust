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
const sunnyFreezeRosters = [
    [named('Milotic', ['Ice Beam'])],
    [{...named('Ninetales', ['Recover']), ability:'Drought'}],
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
const sameChoiceItemRosters = [
    [{...named('Jolteon', ['Tackle', 'Protect']), item:'Choice Scarf'}],
    [{...named('Vaporeon', ['Trick', 'Recover']), item:'Choice Scarf'}],
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
const sleepClauseRosters = [
    [{...named('Slowbro', ['Spore', 'Tackle', 'Yawn', 'Recover']), evs: {spe: 8}}, named('Vaporeon', ['Recover'])],
    [named('Snorlax', ['Rest']), named('Umbreon', ['Recover']), named('Glaceon', ['Recover'])],
];
const sleepTerrainRosters = [[{...sleepClauseRosters[0][0], moves: ['Spore', 'Yawn', 'Misty Terrain', 'Recover']},
    sleepClauseRosters[0][1]], sleepClauseRosters[1]];
const openSheetRosters = [[
    {...rosters[0][0], name: 'Secret nickname', item: 'Choice Band', ability: 'Guts', nature: 'Adamant',
        evs: {atk: 32, spe: 32, hp: 2}, shiny: true, level: 49, teraType: 'Fire'},
    {...named('Rotom-Wash', ['Protect', 'Thunderbolt'], 'N'), ability: 'Levitate', nature: 'Timid'},
    ...rosters[0].slice(2),
], rosters[1]];
const wishRosters = [
    [named('Vaporeon', ['Wish']), named('Glaceon', ['Wish', 'Recover'])],
    [named('Snorlax', ['Tackle']), named('Umbreon', ['Recover'])],
];
const tailwindRosters = [
    [named('Whimsicott', ['Tailwind', 'Recover']), named('Snorlax', ['Tackle'])],
    [named('Umbreon', ['Tackle']), named('Espeon', ['Recover', 'Court Change'])],
];
const hitCountPowerRosters = [
    [{...named('Basculegion', ['Rage Fist', 'Last Respects']), ability: 'Adaptability'},
        named('Shedinja', ['Tackle'], 'N'), named('Vaporeon', ['Recover'])],
    [named('Umbreon', ['Bite', 'Recover']), named('Glaceon', ['Recover'])],
];
const switchAbilityRosters = [
    [{...named('Alomomola', ['Flip Turn', 'Recover']), ability: 'Regenerator'},
        {...named('Blissey', ['Recover']), ability: 'Natural Cure'}],
    [named('Umbreon', ['Toxic', 'Tackle', 'Dragon Tail', 'Recover']), named('Glaceon', ['Recover'])],
];
const contactStatusRosters = [
    [{...named('Umbreon', ['Tackle', 'Shadow Ball']), item: 'Lum Berry'},
        named('Snorlax', ['Tackle', 'Shadow Ball'])],
    [{...named('Ampharos', ['Recover']), ability: 'Static'},
        {...named('Clodsire', ['Recover']), ability: 'Poison Point'}],
];
const flashDryRosters = [
    [{...named('Ninetales', ['Flamethrower', 'Recover']), ability: 'Flash Fire', item: 'Leftovers'},
        {...named('Heliolisk', ['Water Gun', 'Sunny Day', 'Rain Dance', 'Tackle']), ability: 'Dry Skin', item: 'Magnet'},
        named('Flareon', ['Tackle'])],
    [{...named('Umbreon', ['Tackle', 'Will-O-Wisp', 'Water Gun', 'Flamethrower']), item: 'Leftovers'},
        {...named('Banette', ['Protect', 'Recover']), ability: 'Frisk', item: 'Rocky Helmet'}],
];
const keenRecklessRosters = [
    [{...named('Staraptor', ['Brave Bird', 'Sand Attack', 'Double Team']), ability: 'Reckless'}, named('Vaporeon', ['Recover'])],
    [{...named('Skarmory', ['Iron Head', 'Roost']), ability: 'Keen Eye'}, named('Glaceon', ['Recover'])],
];
const purifyingRosters = [
    [named('Vaporeon', ['Shadow Ball', 'Toxic', 'Yawn', 'Recover']), named('Glaceon', ['Shadow Claw', 'Recover'])],
    [{...named('Garganacl', ['Salt Cure', 'Rest', 'Recover']), ability: 'Purifying Salt'}, named('Umbreon', ['Recover'])],
];
const disguiseRosters = [
    [{...named('Haxorus', ['Iron Head']), ability: 'Mold Breaker'}, named('Umbreon', ['Iron Head', 'Recover'])],
    [{...named('Mimikyu', ['Recover', 'Shadow Claw']), ability: 'Disguise', shiny: true, item: 'Leftovers'}, named('Glaceon', ['Recover'])],
];
const disguiseMultihitRosters = [
    [{...named('Cloyster', ['Icicle Spear']), ability: 'Skill Link'}, named('Vaporeon', ['Recover'])],
    disguiseRosters[1],
];
const spinRosters = [
    [named('Vaporeon', ['Rapid Spin', 'Recover', 'Detect']), named('Glaceon', ['Recover'])],
    [named('Ferrothorn', ['Stealth Rock', 'Sticky Web', 'Spikes', 'Leech Seed']), named('Gengar', ['Recover'])],
];
const statusDragRosters = [
    [named('Umbreon', ['Whirlwind', 'Roar', 'Detect']), named('Vaporeon', ['Recover'])],
    [{...named('Octillery', ['Recover']), ability: 'Suction Cups'},
        {...named('Mr. Mime', ['Recover']), ability: 'Soundproof'}, named('Glaceon', ['Recover'])],
];
const dampRosters = [
    [{...named('Golem', ['Explosion', 'Misty Explosion', 'Mind Blown']), ability: 'Sturdy'},
        named('Flareon', ['Tackle', 'Protect']), named('Glaceon', ['Recover'])],
    [{...named('Swampert', ['Recover']), ability: 'Damp'}, named('Gengar', ['Recover']), named('Umbreon', ['Recover'])],
];
const aftermathRosters = [
    [{...named('Machamp', ['High Horsepower', 'Recover']), item: 'Choice Band', evs: {atk: 32}, nature: 'Adamant'},
        named('Glaceon', ['Recover'])],
    [{...named('Stunky', ['Recover']), ability: 'Aftermath'}, named('Umbreon', ['Recover'])],
];
const dampAftermathRosters = [
    [aftermathRosters[0][0], {...named('Golduck', ['Recover']), ability: 'Damp'}],
    [aftermathRosters[1][0], named('Vaporeon', ['Recover']), aftermathRosters[1][1]],
];
const chestoRestRosters = [
    [{...named('Vaporeon', ['Rest', 'Recover']), item: 'Chesto Berry'}, named('Glaceon', ['Recover'])],
    [named('Snorlax', ['Tackle']), named('Umbreon', ['Recover'])],
];
const randomLevelRosters = [
    [{...named('Zoroark', ['Dark Pulse']), ability: 'Illusion', level: 87},
        {...named('Flareon', ['Recover']), level: 84}],
    [{...named('Umbreon', ['Tackle']), level: 86}, {...named('Glaceon', ['Recover']), level: 89}],
];
const soakRosters = [
    [named('Vaporeon', ['Soak', 'Thunderbolt', 'Grass Knot', 'Recover']), named('Glaceon', ['Recover'])],
    [named('Skarmory', ['Iron Head', 'Recover']), {...named('Bouffalant', ['Recover']), ability: 'Sap Sipper'},
        {...named('Vaporeon', ['Recover']), ability: 'Water Absorb'}],
];
const ragingBullRosters = [
    [{...named('Tauros-Paldea-Aqua', ['Raging Bull', 'Recover'], 'M'), item: 'Mystic Water'}, named('Glaceon', ['Recover'])],
    [{...named('Persian-Alola', ['Reflect', 'Body Press', 'Recover']), ability: 'Fur Coat', item: 'Black Belt'},
        named('Vaporeon', ['Recover'])],
];
const corrosionRosters = [
    [{...named('Salazzle', ['Toxic', 'Parting Shot']), ability: 'Corrosion'}, named('Glaceon', ['Recover'])],
    [{...named('Kingambit', ['Recover']), ability: 'Defiant'}, named('Vaporeon', ['Recover'])],
];
const contraryRosters = [
    [{...named('Serperior', ['Leaf Storm', 'Swords Dance', 'Recover']), ability: 'Contrary'}, named('Glaceon', ['Recover'])],
    [{...named('Snorlax', ['Growl', 'Recover']), ability: 'Intimidate'}, named('Vaporeon', ['Recover'])],
];
const critRosters = [
    [{...named('Kingdra', ['Focus Energy', 'Water Gun']), ability: 'Sniper', item: 'Scope Lens'}, named('Glaceon', ['Recover'])],
    [named('Umbreon', ['Recover', 'Tackle']), {...named('Cloyster', ['Recover']), ability: 'Shell Armor'}],
];
const mercilessRosters = [
    [{...named('Toxapex', ['Toxic', 'Water Gun']), ability: 'Merciless'}, named('Glaceon', ['Recover'])],
    critRosters[1],
];
const focusPunchRosters = [
    [named('Snorlax', ['Focus Punch', 'Recover']), named('Glaceon', ['Recover'])],
    [named('Toxapex', ['Fake Out', 'Recover', 'Tackle', 'Toxic']), named('Vaporeon', ['Recover'])],
];
const doublesFocusPunchRosters = [
    [named('Snorlax', ['Focus Punch']), named('Glaceon', ['Focus Punch'])],
    [named('Toxapex', ['Tackle']), named('Vaporeon', ['Recover'])],
];
const reviveBenchRosters = [
    [named('Electrode', ['Explosion', 'Tackle']), named('Pawmot', ['Revival Blessing', 'Tackle'])],
    [named('Gengar', ['Recover']), named('Vaporeon', ['Recover'])],
];
const reviveActiveRosters = [
    [named('Magikarp', ['Tackle']), named('Pawmot', ['Revival Blessing', 'Tackle'])],
    [named('Jolteon', ['Thunderbolt']), named('Vaporeon', ['Protect', 'Water Gun'])],
];
const chillyRosters = [
    [named('Slowking', ['Chilly Reception', 'Recover']), named('Glaceon', ['Recover'])],
    [named('Vaporeon', ['Fake Out', 'Water Gun']), named('Umbreon', ['Recover'])],
];
const fickleBeamRosters = [
    [named('Hydrapple', ['Fickle Beam']), named('Glaceon', ['Recover'])],
    [named('Blissey', ['Recover']), named('Vaporeon', ['Recover'])],
];
const statusPowerRosters = [
    [{...named('Qwilfish', ['Barb Barrage', 'Hex', 'Toxic', 'Venoshock']), ability: 'Pressure'}, named('Glaceon', ['Recover'])],
    [named('Umbreon', ['Haze', 'Calm Mind', 'Recover']), named('Vaporeon', ['Recover'])],
];
const syncRosters = [
    [{...named('Espeon', ['Will-O-Wisp', 'Recover']), item: 'Lum Berry', ability: 'Synchronize'},
        named('Gengar', ['Toxic'])],
    [{...named('Umbreon', ['Recover']), ability: 'Synchronize'},
        {...named('Hypno', ['Recover']), ability: 'Synchronize'}],
];
const syncLimberRosters = [
    [{...named('Persian', ['Thunder Wave']), ability: 'Limber'}, named('Vaporeon', ['Recover'])],
    [{...named('Umbreon', ['Recover']), ability: 'Synchronize'}, named('Espeon', ['Thunder Wave'])],
];
const mortalSpinRosters = [
    [named('Glimmora', ['Mortal Spin', 'Recover']), named('Glaceon', ['Recover'])],
    [named('Ferrothorn', ['Leech Seed', 'Protect']), named('Vaporeon', ['Stealth Rock', 'Protect', 'Recover'])],
];
const substituteRosters = [
    [{...named('Vaporeon', ['Substitute', 'Recover', 'Tackle']), nature: 'Timid', item: 'Rocky Helmet'}, named('Glaceon', ['Recover'])],
    [named('Umbreon', ['Toxic', 'Tackle', 'Hyper Voice', 'Recover']), named('Espeon', ['Recover'])],
];
const shedTailRosters = [
    [named('Orthworm', ['Shed Tail', 'Recover']), named('Glaceon', ['Recover', 'Protect'])],
    [named('Snorlax', ['Tackle', 'Recover', 'Hyper Voice']), named('Espeon', ['Recover'])],
];
const mixedShieldRosters = [
    [named('Vaporeon', ['Surf']), named('Flareon', ['Recover'])],
    [named('Espeon', ['Substitute']), named('Goodra', ['Recover'])],
];
const shieldFocusRosters = [
    [named('Pawmot', ['Substitute', 'Focus Punch']), named('Glaceon', ['Recover'])],
    [named('Toxapex', ['Tackle']), named('Espeon', ['Recover'])],
];
const multiShieldRosters = [
    [substituteRosters[0][0], substituteRosters[0][1]],
    [{...named('Cloyster', ['Icicle Spear']), ability: 'Skill Link', nature: 'Brave'},
        {...named('Noivern', ['Tackle']), ability: 'Infiltrator'}],
];
const shieldSecondariesRosters = [
    [substituteRosters[0][0], substituteRosters[0][1]],
    [named('Torkoal', ['Mystical Fire', 'Rapid Spin', 'Flame Charge', 'Recover']), named('Espeon', ['Recover'])],
];
const doubleShockRosters = [
    [named('Pawmot', ['Double Shock', 'Recover']), named('Glaceon', ['Recover'])],
    [named('Snorlax', ['Protect', 'Thunder Wave', 'Recover']), named('Umbreon', ['Recover'])],
];
const burnUpRosters = [
    [named('Arcanine', ['Burn Up', 'Recover']), named('Glaceon', ['Recover'])],
    [named('Vaporeon', ['Will-O-Wisp', 'Recover']), named('Umbreon', ['Recover'])],
];
const exitRosters = [
    [named('Darmanitan', ['U-turn', 'Flare Blitz']), named('Glaceon', ['Recover'])],
    [{...named('Wimpod', ['Recover']), ability: 'Wimp Out'}, named('Umbreon', ['Recover'])],
];
const exitBerryRosters = [
    exitRosters[0],
    [{...named('Golisopod', ['Recover']), ability: 'Emergency Exit', item: 'Sitrus Berry'}, named('Umbreon', ['Recover'])],
];
const sparklingAriaRosters = [
    [{...named('Primarina', ['Hyper Voice', 'Sparkling Aria']), ability: 'Liquid Voice'},
        named('Rotom-Wash', ['Will-O-Wisp', 'Recover'])],
    [{...named('Vaporeon', ['Recover']), ability: 'Water Absorb'}, named('Sylveon', ['Will-O-Wisp', 'Recover'])],
];
const waterBubbleRosters = [
    [{...named('Araquanid', ['Water Gun', 'Recover']), ability: 'Water Bubble'},
        {...named('Kommo-o', ['Recover']), ability: 'Bulletproof'}],
    [named('Vaporeon', ['Flamethrower', 'Will-O-Wisp', 'Shadow Ball', 'Recover']), named('Umbreon', ['Recover'])],
];
const counterRosters = [
    [named('Blissey', ['Counter', 'Mirror Coat', 'Recover']), named('Sylveon', ['Recover'])],
    [named('Vaporeon', ['Water Gun', 'Tackle', 'Recover']), named('Umbreon', ['Recover'])],
];
const doublesCounterRosters = [
    counterRosters[0],
    [named('Electrode', ['Tackle']), counterRosters[1][0]],
];
const faintedCounterRosters = [
    [counterRosters[0][0], {...named('Garchomp', ['Stomping Tantrum']), ability:'Sand Veil', nature:'Adamant', evs:{atk:32}}],
    [named('Electrode', ['Tackle']), named('Umbreon', ['Recover']), named('Glaceon', ['Recover'])],
];
const soulRosters = [
    [{...named('Kommo-o', ['Clangorous Soul', 'Recover']), item: 'Sitrus Berry'}, named('Espeon', ['Recover'])],
    [named('Vaporeon', ['Recover']), named('Umbreon', ['Recover'])],
];
const steelBeamRosters = [
    [named('Duraludon', ['Steel Beam']), named('Espeon', ['Recover'])],
    [named('Vaporeon', ['Protect', 'Recover']), named('Umbreon', ['Recover'])],
];
const quickClawRosters = [
    [{...named('Snorlax', ['Recover']), item: 'Quick Claw'}, named('Espeon', ['Recover'])],
    [named('Vaporeon', ['Recover', 'Quick Attack']), named('Umbreon', ['Recover'])],
];
const lightBallRosters = [
    [{...named('Pikachu', ['Quick Attack', 'Thunderbolt']), item: 'Light Ball'}, named('Espeon', ['Recover'])],
    [named('Umbreon', ['Recover']), named('Vaporeon', ['Recover'])],
];
const speedAbilityRosters = [
    [{...named('Lucario', ['Tackle', 'Recover']), ability: 'Steadfast'}, named('Espeon', ['Recover'])],
    [{...named('Raichu-Alola', ['Fake Out', 'Electric Terrain', 'Tackle']), ability: 'Surge Surfer'}, named('Umbreon', ['Recover'])],
];
const bindingRosters = [
    [{...named('Toxapex', ['Infestation', 'Recover', 'Tidy Up']), item: 'Binding Band'}, named('Espeon', ['Recover'])],
    [named('Umbreon', ['Recover', 'Substitute', 'Rapid Spin']), named('Vaporeon', ['Recover'])],
];
const bindingExpiryRosters = [
    [named('Toxapex', ['Infestation', 'Recover']), bindingRosters[0][1]], bindingRosters[1],
];
const screenCleanRosters = [
    [named('Vaporeon', ['Reflect', 'Light Screen', 'Recover']),
        {...named('Mr. Mime', ['Recover']), ability:'Screen Cleaner'}],
    [named('Umbreon', ['Light Screen', 'Reflect', 'Recover']), named('Espeon', ['Recover'])],
];
const aromaRosters = [
    [named('Gardevoir', ['Taunt', 'Encore', 'Disable']), {...named('Haxorus', ['Recover', 'Taunt']), ability:'Mold Breaker'}],
    [{...named('Alcremie', ['Recover']), ability:'Aroma Veil'}, named('Umbreon', ['Tackle', 'Recover'])],
];
const healerRosters = [
    [named('Gardevoir', ['Will-O-Wisp']), named('Umbreon', ['Recover'])],
    [{...named('Blissey', ['Recover']), ability:'Healer'}, named('Glaceon', ['Recover'])],
];
const unawareRosters = [
    [{...named('Clefable', ['Swords Dance', 'Tackle', 'Recover']), ability:'Unaware'}, named('Vaporeon', ['Recover'])],
    [named('Umbreon', ['Iron Defense', 'Swords Dance', 'Tackle', 'Double Team']),
        {...named('Haxorus', ['Swords Dance', 'Tackle']), ability:'Mold Breaker'}],
];
const doubleUnawareRosters = [unawareRosters[0], [unawareRosters[0][0], named('Espeon', ['Recover'])]];
const batonShieldRosters = [
    [named('Jolteon', ['Calm Mind', 'Substitute', 'Baton Pass']), named('Glaceon', ['Water Gun', 'Recover'])],
    [named('Umbreon', ['Toxic', 'Recover', 'Tackle']), named('Vaporeon', ['Recover'])],
];
const batonBoundRosters = [
    [named('Vaporeon', ['Recover', 'Baton Pass']), named('Umbreon', ['Recover'])],
    [named('Ferrothorn', ['Leech Seed', 'Salt Cure', 'Recover', 'Infestation']), named('Mamoswine', ['Recover'])],
];
const rainDishRosters = [
    [{...named('Ludicolo', ['Rain Dance', 'Recover']), ability:'Rain Dish'}, named('Espeon', ['Recover'])],
    [named('Umbreon', ['Tackle', 'Recover']), {...named('Golduck', ['Tackle', 'Recover']), ability:'Cloud Nine'}],
];
const snowCloakRosters = [
    [{...named('Froslass', ['Snowscape', 'Recover']), ability:'Snow Cloak'}, named('Vaporeon', ['Recover'])],
    [named('Umbreon', ['Water Gun', 'Recover']), {...named('Haxorus', ['Water Gun']), ability:'Mold Breaker'}],
];
const sacredSwordRosters = [
    [named('Gallade', ['Swords Dance', 'Sacred Sword', 'Recover']), named('Vaporeon', ['Recover'])],
    [named('Clefable', ['Iron Defense', 'Double Team', 'Reflect', 'Recover']), named('Umbreon', ['Recover'])],
];
const snipeShotRosters = [
    [named('Inteleon', ['Snipe Shot', 'Water Gun']), named('Umbreon', ['Recover'])],
    [named('Togekiss', ['Follow Me', 'Recover']), named('Sylveon', ['Recover'])],
];
const yawnAbilityRosters = [
    [named('Espeon', ['Yawn', 'Spore', 'Sunny Day', 'Rain Dance']), named('Umbreon', ['Recover'])],
    [{...named('Lycanroc-Midnight', ['Recover']), ability:'Vital Spirit'},
        {...named('Noctowl', ['Recover']), ability:'Insomnia'},
        {...named('Meganium', ['Recover']), ability:'Leaf Guard'}],
];
const earlyBirdRosters = [
    [{...named('Houndoom', ['Recover', 'Rest', 'Tackle']), ability:'Early Bird'}, named('Umbreon', ['Recover'])],
    [named('Jolteon', ['Tackle', 'Recover', 'Spore']), named('Vaporeon', ['Recover'])],
];
const scrappyRosters = [
    [{...named('Exploud', ['Tackle', 'Power-Up Punch']), ability:'Scrappy'}, named('Vaporeon', ['Recover'])],
    [{...named('Arbok', ['Recover']), ability:'Intimidate'}, named('Froslass', ['Recover'])],
];
const secondaryProtectionRosters = [
    [named('Umbreon', ['Snarl', 'Fiery Dance']), named('Espeon', ['Recover'])],
    [{...named('Ribombee', ['Recover']), ability:'Shield Dust'},
        {...named('Vaporeon', ['Recover']), item:'Covert Cloak'}],
];
const telepathyRosters = [
    [named('Garchomp', ['Earthquake', 'Tail Whip']),
        {...named('Gardevoir', ['Recover']), ability:'Telepathy'},
        {...named('Haxorus', ['Earthquake']), ability:'Mold Breaker'}],
    [named('Vaporeon', ['Recover']), named('Umbreon', ['Recover'])],
];
const attackHazardRosters = [
    [named('Samurott-Hisui', ['Stone Axe', 'Ceaseless Edge', 'Recover']), named('Vaporeon', ['Recover'])],
    [named('Jolteon', ['Protect', 'Substitute', 'Recover']), named('Umbreon', ['Recover'])],
];
const repeatedHammerRosters = [
    [named('Tinkaton', ['Gigaton Hammer', 'Recover']), named('Vaporeon', ['Recover'])],
    [named('Jolteon', ['Protect', 'Encore', 'Recover']), named('Espeon', ['Recover'])],
];
const bloodMoonRosters = [
    [{...named('Ursaluna-Bloodmoon', ['Blood Moon', 'Recover']), ability:"Mind's Eye"}, named('Umbreon', ['Recover'])],
    [named('Drifblim', ['Protect', 'Recover', 'Sand Attack']), named('Vaporeon', ['Recover'])],
];
const mimicryRosters = [
    [{...named('Stunfisk-Galar', ['Recover']), ability:'Mimicry'}, named('Umbreon', ['Recover'])],
    [named('Vaporeon', ['Electric Terrain', 'Misty Terrain', 'Recover', 'Water Gun']), named('Espeon', ['Recover'])],
];
const angerPointRosters = [
    [{...named('Tauros', ['Substitute', 'Recover']), ability:'Anger Point'}, named('Umbreon', ['Recover'])],
    [named('Espeon', ['Storm Throw', 'Recover']), named('Vaporeon', ['Recover'])],
];
const eelevateRosters = [
    [{...named('Eelektross', ['Thunderbolt', 'Recover']), ability:'Levitate', item:'Eelektrossite'},
        named('Umbreon', ['Recover'])],
    [named('Magikarp', ['Earthquake', 'Recover'], 'M'),
        {...named('Haxorus', ['Earthquake']), ability:'Mold Breaker'}],
];
const recycleRosters = [
    [{...named('Sneasler', ['Recycle', 'Recover', 'Tackle']), ability:'Unburden',
        item:'Sitrus Berry', evs:{hp:1}}, named('Umbreon', ['Recover'])],
    [named('Jolteon', ['Super Fang', 'Recover', 'Knock Off']), named('Vaporeon', ['Recover'])],
];
const poppedBalloonRosters = [
    [{...named('Jolteon', ['Substitute', 'Recycle', 'Recover']), item:'Air Balloon'}, named('Umbreon', ['Recover'])],
    [named('Samurott-Hisui', ['Stone Axe', 'Recover']), named('Vaporeon', ['Recover'])],
];
const defogRosters = [
    [named('Vaporeon', ['Defog', 'Reflect', 'Spikes', 'Grassy Terrain']), named('Espeon', ['Recover'])],
    [{...named('Umbreon', ['Stealth Rock', 'Light Screen', 'Substitute', 'Recover']), evs:{spe:4}},
        named('Jolteon', ['Recover'])],
];
const hospitalityRosters = [
    [named('Vaporeon', ['Water Gun', 'Recover']), named('Umbreon', ['Recover']),
        {...named('Sinistcha', ['Pollen Puff']), ability:'Hospitality'}],
    [named('Jolteon', ['Shadow Ball', 'Recover']), named('Glaceon', ['Recover'])],
];
const overlordRosters = [
    [named('Magikarp', ['Explosion'], 'M'),
        {...named('Kingambit', ['Water Gun', 'Recover']), ability:'Supreme Overlord'},
        named('Vaporeon', ['Explosion'])],
    [named('Dusclops', ['Recover']), named('Gengar', ['Recover'])],
];
const receiverRosters = [
    [{...named('Passimian', ['Protect', 'Water Gun']), ability:'Receiver'},
        {...named('Kingambit', ['Explosion']), ability:'Supreme Overlord'},
        named('Vaporeon', ['Recover'])],
    [named('Dusclops', ['Recover']), named('Gengar', ['Recover'])],
];
const receiverUnnerveRosters = [
    [receiverRosters[0][0], {...receiverRosters[0][1], ability:'Unnerve'}, receiverRosters[0][2]],
    receiverRosters[1],
];
const healingWishRosters = [
    [named('Vaporeon', ['Recover']), named('Espeon', ['Healing Wish']), named('Jolteon', ['Recover'])],
    [named('Umbreon', ['Will-O-Wisp', 'Recover', 'Stealth Rock']), named('Glaceon', ['Recover'])],
];
const lunarDanceRosters = [
    [named('Vaporeon', ['Recover']), named('Cresselia', ['Lunar Dance'])],
    [named('Umbreon', ['Recover']), named('Glaceon', ['Recover'])],
];
const ripenRosters = [
    [{...named('Dipplin', ['Growl', 'Recover']), ability:'Ripen', item:'Sitrus Berry', evs:{hp:1}},
        {...named('Appletun', ['Recover']), ability:'Ripen', item:'Yache Berry'},
        {...named('Flapple', ['Growl']), ability:'Ripen', item:'Leftovers'}],
    [named('Flareon', ['Super Fang', 'Ice Shard', 'Recover']), named('Jolteon', ['Recover'])],
];
const beakBlastRosters = [
    [named('Toucannon', ['Beak Blast', 'Recover']), named('Glaceon', ['Recover'])],
    [named('Vaporeon', ['Tackle', 'Recover']), named('Jolteon', ['Water Gun', 'Tackle', 'Recover'])],
];
const curseRosters = [
    [named('Spiritomb', ['Curse', 'Recover']), named('Umbreon', ['Recover'])],
    [named('Vaporeon', ['Substitute', 'Recover']), named('Jolteon', ['Recover'])],
];
const curseDoublesRosters = [
    [named('Vaporeon', ['Curse', 'Recover']), named('Gengar', ['Curse', 'Recover'])],
    [named('Umbreon', ['Recover']), named('Espeon', ['Recover'])],
];
const allyAbilitiesRosters = [
    [{...named('Ampharos', ['Calm Mind', 'Water Gun']), ability:'Plus'},
        {...named('Manectric', ['Growl']), ability:'Minus'},
        {...named('Slowking-Galar', ['Recover']), ability:'Curious Medicine'}],
    [named('Vaporeon', ['Recover']), named('Umbreon', ['Recover'])],
];
const acupressureRosters = [
    [named('Pincurchin', ['Acupressure']), named('Arbok', ['Recover'])],
    [named('Umbreon', ['Recover']), named('Glaceon', ['Recover'])],
];
const magneticFluxRosters = [
    [{...named('Ampharos', ['Magnetic Flux']), ability:'Plus'},
        {...named('Manectric', ['Recover']), ability:'Minus'}],
    [named('Umbreon', ['Magnetic Flux']), named('Glaceon', ['Recover'])],
];
const futureSightRosters = [
    [{...named('Slowbro', ['Future Sight', 'Recover']), item:'Choice Specs'}, named('Jolteon', ['Recover'])],
    [named('Umbreon', ['Recover']), named('Vaporeon', ['Recover', 'Protect'])],
];
const doubleFutureRosters = [
    [named('Espeon', ['Future Sight', 'Recover']), named('Metagross', ['Doom Desire', 'Recover'])],
    [named('Vaporeon', ['Substitute', 'Recover']), named('Umbreon', ['Recover'])],
];
const faintedFutureRosters = [
    [named('Espeon', ['Future Sight']), named('Umbreon', ['Recover'])],
    [{...named('Tyranitar', ['Crunch']), ability:'Unnerve', nature:'Adamant', evs:{atk:32}},
        named('Vaporeon', ['Recover'])],
];
const belchRosters = [
    [{...named('Swalot', ['Recover', 'Belch']), item:'Lum Berry'}, named('Vaporeon', ['Recover'])],
    [named('Umbreon', ['Will-O-Wisp', 'Recover']), named('Espeon', ['Recover'])],
];
const imposterRosters = [
    [{...named('Ditto', ['Transform'], 'N'), item:'Leftovers', ability:'Imposter'}, named('Glaceon', ['Recover'])],
    [{...named('Flareon', ['Tackle', 'Focus Energy', 'Recover']), ability:'Flash Fire'}, named('Vaporeon', ['Recover'])],
];
const transformRosters = [
    [{...named('Mew', ['Transform', 'Recover'], 'N'), item:'Choice Scarf'}, named('Umbreon', ['Recover'])],
    [{...named('Vaporeon', ['Substitute', 'Tackle', 'Recover']), ability:'Water Absorb'}, named('Jolteon', ['Recover'])],
];
const imposterDoublesRosters = [
    [imposterRosters[0][0], named('Vaporeon', ['Recover'])],
    [named('Leafeon', ['Recover']), named('Glaceon', ['Ice Shard', 'Recover'])],
];
const imposterFaintRosters = [
    [imposterRosters[0][0], named('Vaporeon', ['Recover'])],
    [{...named('Chandelure', ['Flamethrower']), item:'Choice Scarf', nature:'Modest', evs:{spa:32, spe:32}},
        named('Leafeon', ['Swords Dance', 'Leaf Blade'])],
];
const dartRosters = [
    [named('Dragapult', ['Dragon Darts']), named('Umbreon', ['Recover'])],
    [named('Glaceon', ['Recover', 'Protect', 'Follow Me']), named('Vaporeon', ['Recover']), named('Sylveon', ['Recover'])],
];
const superEffectiveGuardRosters = [
    [named('Vaporeon', ['Water Gun']), {...named('Haxorus', ['Water Gun', 'Shadow Ball']), ability:'Mold Breaker'}],
    [{...named('Camerupt', ['Recover']), ability:'Solid Rock'},
        {...named('Mr. Mime', ['Recover']), ability:'Filter'},
        {...named('Necrozma-Dawn-Wings', ['Recover'], 'N'), ability:'Prism Armor'}],
];
const defenseHookRosters = [
    [named('Umbreon', ['Tail Whip', 'Fake Tears', 'Bite']), named('Vaporeon', ['Recover'])],
    [{...named('Mandibuzz', ['Recover']), ability:'Big Pecks'},
        {...named('Gallade', ['Recover'], 'M'), ability:'Justified'}],
];
const specialPhysicalDefenseRosters = [
    [named('Espeon', ['Psyshock', 'Psychic', 'Psystrike', 'Secret Sword'])],
    [{...named('Furfrou', ['Cotton Guard', 'Recover']), ability:'Fur Coat'}],
];
const situationalPowerRosters = [
    [{...named('Hawlucha', ['Acrobatics', 'Hard Press']), item:'Sitrus Berry'}],
    [named('Vaporeon', ['Seismic Toss', 'Recover'])],
];
const risingVoltageRosters = [
    [named('Bellibolt', ['Electric Terrain', 'Rising Voltage'])],
    [named('Umbreon', ['Recover']), {...named('Rotom', ['Recover'], 'N'), ability:'Levitate'}],
];
const contactSpeedRosters = [
    [{...named('Mawile', ['Tackle']), ability:'Hyper Cutter'}],
    [{...named('Goodra', ['Growl']), ability:'Gooey'}],
];
const stallRosters = [
    [{...named('Sableye', ['Tackle', 'Quick Attack'], 'M'), ability:'Stall'}],
    [named('Snorlax', ['Tackle'])],
];
const hydrationRosters = [
    [{...named('Vaporeon', ['Rain Dance', 'Recover']), ability:'Hydration', item:'Leftovers'}],
    [named('Umbreon', ['Toxic', 'Tackle'])],
];
const confuseRayRosters = [
    [named('Espeon', ['Confuse Ray', 'Tackle', 'Misty Terrain'])],
    [{...named('Spinda', ['Recover']), ability:'Tangled Feet'},
        {...named('Mudsdale', ['Recover']), ability:'Own Tempo'}],
];
const fluffyRosters = [
    [named('Flareon', ['Tackle', 'Flamethrower', 'Flame Charge'])],
    [{...named('Bewear', ['Recover']), ability:'Fluffy'}],
];
const punkRockRosters = [
    [{...named('Toxtricity', ['Hyper Voice', 'Water Gun']), ability:'Punk Rock'}],
    [{...named('Sylveon', ['Hyper Voice', 'Recover']), ability:'Pixilate'}],
];
const spicySprayRosters = [
    [named('Vaporeon', ['Water Gun', 'Tackle'])],
    [{...named('Scovillain', ['Growth', 'Recover']), ability:'Chlorophyll', item:'Scovillainite'}],
];
const bellyDrumRosters = [
    [{...named('Azumarill', ['Belly Drum']), ability:'Huge Power', item:'Sitrus Berry'},
        {...named('Malamar', ['Belly Drum']), ability:'Contrary'}],
    [named('Umbreon', ['Tackle'])],
];
const quashRosters = [
    [{...named('Sableye', ['Quash']), ability:'Prankster'}, named('Umbreon', ['Tackle'])],
    [named('Jolteon', ['Tackle', 'Quick Attack']), named('Vaporeon', ['Water Gun'])],
];
const theftRosters = [
    [named('Greninja', ['Thief']), named('Linoone', ['Covet'])],
    [{...named('Muk', ['Recover']), item:'Leftovers', ability:'Sticky Hold'},
        {...named('Umbreon', ['Recover']), item:'Rocky Helmet'},
        {...named('Vaporeon', ['Recover']), item:'Mystic Water'}],
];
const pickpocketRosters = [
    [{...named('Flareon', ['Tackle', 'U-turn']), item:'Leftovers'}, named('Espeon', ['Recover'])],
    [{...named('Weavile', ['Recover']), ability:'Pickpocket'}],
];
const teamHealingRosters = [
    [named('Vaporeon', ['Life Dew', 'Water Gun']),
        {...named('Clawitzer', ['Heal Pulse']), ability:'Mega Launcher'}],
    [named('Jolteon', ['Tackle', 'Tail Whip']), named('Umbreon', ['Tackle', 'Tail Whip'])],
];
const finalGambitRosters = [
    [named('Annihilape', ['Final Gambit']), named('Vaporeon', ['Recover'])],
    [named('Dusclops', ['Protect', 'Recover']), named('Umbreon', ['Recover'])],
];
const innardsOutRosters = [
    [{...named('Victreebel', ['Growth']), ability:'Chlorophyll', item:'Victreebelite'}, named('Espeon', ['Recover'])],
    [{...named('Chandelure', ['Flamethrower']), item:'Choice Specs', nature:'Modest', evs:{spa:32}},
        named('Vaporeon', ['Recover'])],
];
const dangerSenseRosters = [
    [{...named('Whiscash', ['Recover']), ability:'Anticipation'},
        {...named('Hypno', ['Recover']), ability:'Forewarn'}],
    [named('Leafeon', ['Leaf Blade', 'Protect']), named('Flareon', ['Flamethrower', 'Protect'])],
];
const seedSowerRosters = [
    [named('Espeon', ['Tackle'])],
    [{...named('Arboliva', ['Recover']), ability:'Seed Sower', item:'Terrain Extender'}],
];
const forecastRosters = [
    [{...named('Castform', ['Rain Dance', 'Weather Ball', 'Sunny Day', 'Snowscape']), ability:'Forecast'}],
    [named('Vaporeon', ['Recover']), {...named('Drampa', ['Recover']), ability:'Cloud Nine'}],
];
const hungerSwitchRosters = [
    [{...named('Morpeko', ['Aura Wheel']), ability:'Hunger Switch'}, named('Umbreon', ['Recover'])],
    [{...named('Jolteon', ['Recover']), ability:'Volt Absorb'}],
];
const phantomForceRosters = [
    [named('Dragapult', ['Phantom Force']), named('Glaceon', ['Water Gun'])],
    [named('Umbreon', ['Foul Play', 'Protect']), named('Vaporeon', ['Shadow Ball', 'Recover'])],
];
const skyAttackRosters = [
    [{...named('Noivern', ['Sky Attack']), item:'Power Herb'}],
    [named('Vaporeon', ['Recover'])],
];
const personalGuardsRosters = [
    [named('Flareon', ['Tackle'])],
    [named('Toxapex', ['Baneful Bunker']), named('Cacturne', ['Spiky Shield'])],
];
const unseenFistRosters = [
    [{...named('Golurk', ['Shadow Punch', 'Shadow Ball'], 'N'), item:'Golurkite'}],
    [named('Toxapex', ['Protect', 'Recover'])],
];
const sideGuardRosters = [
    [named('Hitmontop', ['Wide Guard', 'Quick Guard'], 'M'), named('Vaporeon', ['Recover'])],
    [named('Jolteon', ['Swift', 'Quick Attack']), named('Glaceon', ['Icy Wind', 'Feint', 'Recover'])],
];
const retaliationPowerRosters = [
    [named('Avalugg', ['Avalanche', 'Earthquake', 'Temper Flare'])],
    [{...named('Rotom', ['Tackle', 'Recover'], 'N'), ability:'Levitate'}],
];
const coachingRosters = [
    [named('Lucario', ['Coaching', 'Swords Dance']), named('Umbreon', ['Tackle'])],
    [named('Flareon', ['Burning Jealousy']), named('Vaporeon', ['Recover'])],
];
const endureRosters = [
    [{...named('Snorlax', ['Endure']), item:'Focus Sash'}],
    [{...named('Annihilape', ['Close Combat']), item:'Choice Band', nature:'Adamant', evs:{atk:32}}],
];
const quickDrawRosters = [
    [{...named('Slowbro-Galar', ['Tackle', 'Recover']), ability:'Quick Draw', item:'Quick Claw'}],
    [named('Vaporeon', ['Recover'])],
];
const poisonTouchRosters = [
    [{...named('Sneasler', ['Tackle']), ability:'Poison Touch'}],
    [named('Vaporeon', ['Recover'])],
];
const psychicNoiseRosters = [
    [named('Espeon', ['Psychic Noise', 'Calm Mind'])],
    [{...named('Hitmontop', ['Recover', 'Tackle'], 'M'), item:'Sitrus Berry'}],
];
const speedPowerRosters = [
    [{...named('Bronzong', ['Trick Room', 'Gyro Ball'], 'N'), item:'Iron Ball'}],
    [named('Jolteon', ['Electro Ball', 'Recover'])],
];
const terrainPulseRosters = [
    [{...named('Clawitzer', ['Misty Terrain', 'Terrain Pulse']), ability:'Mega Launcher'}],
    [named('Vaporeon', ['Aqua Ring', 'Tackle'])],
];
const upperHandRosters = [
    [named('Hitmontop', ['Upper Hand'], 'M')],
    [named('Jolteon', ['Quick Attack', 'Tackle'])],
];
const steelySpiritRosters = [
    [{...named('Perrserker', ['Metal Claw']), ability:'Steely Spirit'}, named('Lucario', ['Flash Cannon'])],
    [named('Vaporeon', ['Recover']), named('Umbreon', ['Recover'])],
];
const faintedMegaRosters = [
    [{...named('Greninja', ['Ice Beam']), nature:'Modest', evs:{spa:32}, item:'Choice Specs'},
        named('Vaporeon', ['Recover'])],
    [named('Umbreon', ['Recover']), {...named('Pidgeot', ['Recover']), item:'Pidgeotite'}],
];
const fullMoonlightRosters = [
    [named('Umbreon', ['Moonlight', 'Stomping Tantrum']), named('Vaporeon', ['Recover'])],
    [named('Umbreon', ['Recover']), named('Vaporeon', ['Recover'])],
];
const instructRosters = [
    [named('Oranguru', ['Instruct']), named('Jolteon', ['Tackle'])],
    [named('Umbreon', ['Recover']), named('Vaporeon', ['Recover']), named('Flareon', ['Recover'])],
];
const shellSideArmRosters = [
    [named('Slowbro-Galar', ['Shell Side Arm'])],
    [named('Umbreon', ['Recover']), named('Cloyster', ['Recover'])],
];
const sleepTalkRosters = [
    [named('Vaporeon', ['Sleep Talk', 'Water Gun', 'Solar Beam'])],
    [{...named('Mewtwo', ['Spore', 'Recover'], 'N'), ability:'Pressure'}],
];
const sleepTalkRestRosters = [
    [named('Snorlax', ['Sleep Talk', 'Rest'])],
    [named('Vaporeon', ['Tackle'])],
];
const tormentRosters = [
    [named('Umbreon', ['Torment', 'Recover'])],
    [named('Vaporeon', ['Tackle', 'Water Gun'])],
];
const octolockRosters = [
    [named('Grapploct', ['Octolock', 'Recover']), named('Leafeon', ['Recover'])],
    [named('Vaporeon', ['Recover']), named('Froslass', ['Recover'])],
];
const safeguardRosters = [
    [{...named('Meowstic', ['Safeguard', 'Recover'], 'M'), ability:'Prankster'}],
    [named('Gengar', ['Toxic', 'Recover']),
        {...named('Crobat', ['Toxic', 'Recover']), ability:'Infiltrator'}],
];
const replaceAbilityRosters = [
    [{...named('Whimsicott', ['Spore', 'Worry Seed', 'Simple Beam', 'Entrainment']), ability:'Prankster'}],
    [named('Vaporeon', ['Calm Mind']), {...named('Jolteon', ['Calm Mind']), item:'Ability Shield'}],
];
const legacyMoveRosters = [
    [{...named('Pidgeot', ['Return', 'Frustration', 'Hidden Power Ice']), happiness:128, ability:'Keen Eye'},
        {...named('Jolteon', ['Hidden Power']), ivs:{hp:31, atk:30, def:30, spa:31, spd:31, spe:31}}],
    [named('Gliscor', ['Recover'])],
];
const cheekPouchRosters = [
    [{...named('Greedent', ['Tackle', 'Recover']), ability:'Cheek Pouch', item:'Sitrus Berry'}],
    [named('Raticate', ['Super Fang', 'Recover'])],
];
const solarPowerRosters = [
    [{...named('Charizard', ['Dragon Pulse', 'Flamethrower']), ability:'Solar Power', item:'Choice Specs'},
        {...named('Heliolisk', ['Thunderbolt']), ability:'Solar Power', item:'Utility Umbrella'}],
    [{...named('Ninetales', ['Recover']), ability:'Drought'}],
];
const piercingDrillRosters = [
    [{...named('Excadrill', ['Iron Head', 'Earthquake']), ability:'Sand Rush', item:'Excadrite'}],
    [named('Vaporeon', ['Protect', 'Recover', 'Spiky Shield'])],
];
const auraRosters = [
    [{...named('Sylveon', ['Moonblast']), ability:'Fairy Aura'}, named('Umbreon', ['Recover'])],
    [named('Vaporeon', ['Recover']), named('Leafeon', ['Recover']),
        {...named('Froslass', ['Recover']), ability:'Aura Break'}],
];
const stalwartRosters = [
    [{...named('Archaludon', ['Dragon Pulse']), ability:'Stalwart'}, named('Vaporeon', ['Recover'])],
    [named('Clefable', ['Follow Me']), named('Umbreon', ['Recover'])],
];
const stalwartHazardRosters = [
    [{...named('Chandelure', ['Mystical Fire']), ability:'Flash Fire', nature:'Timid', evs:{spa:32, spe:32}},
        named('Mamoswine', ['Snowscape'])],
    [{...named('Archaludon', ['Stealth Rock']), ability:'Stalwart', evs:{spe:16}},
        named('Sinistcha', ['Recover']), named('Vaporeon', ['Recover'])],
];
const rattledRosters = [
    [{...named('Magikarp', ['Tackle']), ability:'Rattled'}],
    [{...named('Mightyena', ['Snarl']), ability:'Intimidate'}],
];
const stenchRosters = [
    [{...named('Skuntank', ['Tackle', 'Bite']), ability:'Stench', nature:'Jolly', evs:{spe:32}}],
    [named('Umbreon', ['Recover']), {...named('Vaporeon', ['Substitute', 'Recover']), item:'Covert Cloak'}],
];
const redCardRosters = [
    [named('Garchomp', ['U-turn']), named('Umbreon', ['Recover'])],
    [{...named('Vaporeon', ['Recover', 'Protect']), item:'Red Card'}],
];
const guardDogCardRosters = [
    [{...named('Mabosstiff', ['Tackle']), ability:'Guard Dog'}, redCardRosters[0][1]],
    redCardRosters[1],
];
const lifeOrbCardRosters = [
    [{...named('Garchomp', ['Stomping Tantrum']), item:'Life Orb'}, redCardRosters[0][1]],
    redCardRosters[1],
];
const floettiteRosters = [
    [{...named('Floette-Eternal', ['Tackle']), ability:'Flower Veil', item:'Floettite'}],
    [named('Umbreon', ['Knock Off'])],
];
const terrainSeedRosters = [
    [named('Vaporeon', ['Grassy Terrain', 'Psychic Terrain', 'Recover'])],
    [{...named('Drifblim', ['Recover']), ability:'Unburden', item:'Grassy Seed'},
        {...named('Glaceon', ['Recover']), item:'Psychic Seed'}],
];
const shackleRosters = [
    [named('Decidueye', ['Spirit Shackle', 'U-turn']), named('Umbreon', ['Recover'])],
    [named('Vaporeon', ['Recover']), named('Glaceon', ['Recover'])],
];
const jawLockRosters = [
    [{...named('Mabosstiff', ['Jaw Lock', 'Recover']), ability:'Guard Dog'}, named('Umbreon', ['Recover'])],
    [{...named('Vaporeon', ['Recover']), item:'Shed Shell'}, named('Spiritomb', ['Recover'])],
];
const ironBallFlingRosters = [
    [{...named('Pinsir', ['Fling', 'Recycle']), item:'Iron Ball', ability:'Mold Breaker'}],
    [named('Vaporeon', ['Protect', 'Recover'])],
];
const kingsRockRosters = [
    [{...named('Tauros', ['Iron Head', 'Fling'], 'M'), ability:'Sheer Force', item:"King's Rock"}],
    [named('Umbreon', ['Recover'])],
];
const longReachRosters = [
    [{...named('Decidueye', ['Tackle']), ability:'Long Reach'}],
    [{...named('Bewear', ['Spiky Shield', 'Recover']), ability:'Fluffy', item:'Rocky Helmet'}],
];
const stockpileRosters = [
    [named('Swalot', ['Stockpile', 'Spit Up', 'Swallow'])],
    [named('Vaporeon', ['Tackle', 'Protect', 'Recover'])],
];
const opportunistRosters = [
    [{...named('Espathra', ['Calm Mind', 'Recover']), ability:'Opportunist'}],
    [{...named('Ninjask', ['Swords Dance', 'Tackle']), ability:'Speed Boost'},
        {...named('Azumarill', ['Belly Drum']), ability:'Huge Power', item:'Sitrus Berry'}],
];
const magnetRiseRosters = [
    [named('Metagross', ['Magnet Rise', 'Recover'], 'N')],
    [named('Nosepass', ['Earthquake', 'Smack Down', 'Gravity', 'Recover'])],
];
const smackDownFlyRosters = [
    [named('Noivern', ['Fly', 'Recover'])],
    [named('Tyranitar', ['Smack Down', 'Earthquake'])],
];
const blockRosters = [
    [named('Snorlax', ['Block', 'Recover'])],
    [named('Vaporeon', ['Substitute', 'Recover']), named('Dusknoir', ['Recover'])],
];
const howlRosters = [
    [named('Arcanine', ['Howl']), {...named('Exploud', ['Recover']), ability:'Soundproof'},
        named('Leafeon', ['Recover'])],
    [named('Umbreon', ['Recover']), named('Glaceon', ['Recover'])],
];
const teeterDanceRosters = [
    [{...named('Lilligant', ['Teeter Dance']), ability:'Chlorophyll'},
        {...named('Golduck', ['Recover']), ability:'Own Tempo'}],
    [named('Umbreon', ['Protect', 'Recover']), named('Vaporeon', ['Recover'])],
];
const marvelScaleRosters = [
    [{...named('Milotic', ['Recover']), ability:'Marvel Scale', item:'Flame Orb'}],
    [named('Vaporeon', ['Tackle'])],
];
const grassPeltRosters = [
    [{...named('Gogoat', ['Grassy Terrain', 'Recover']), ability:'Grass Pelt'}],
    [named('Vaporeon', ['Tackle'])],
];
const quickFeetRosters = [
    [{...named('Ursaring', ['Tackle']), ability:'Quick Feet'}],
    [named('Umbreon', ['Thunder Wave', 'Recover'])],
];
const sandSpitRosters = [
    [{...named('Sandaconda', ['Recover']), ability:'Sand Spit', item:'Smooth Rock'}],
    [named('Vaporeon', ['Water Gun', 'Recover'])],
];
const moodyRosters = [
    [{...named('Glalie', ['Recover']), ability:'Moody'}],
    [{...named('Espathra', ['Recover']), ability:'Opportunist'}],
];
const magmaArmorRosters = [
    [{...named('Camerupt', ['Recover']), ability:'Magma Armor'}],
    [named('Froslass', ['Ice Beam'])],
];
const mementoRosters = [
    [named('Whimsicott', ['Memento']), named('Umbreon', ['Recover'])],
    [{...named('Metagross', ['Protect', 'Recover'], 'N'), ability:'Clear Body'}],
];
const minimizeRosters = [
    [named('Clefable', ['Minimize', 'Recover'])],
    [named('Blissey', ['Recover', 'Body Slam'])],
];
const hazeTieRosters = [
    [named('Vaporeon', ['Haze'])],
    [named('Vaporeon', ['Tackle'])],
];
const megaSpeedTieRosters = [
    [{...named('Eelektross', ['Coil']), item:'Eelektrossite', ability:'Levitate'}],
    [{...named('Bellibolt', ['Icy Wind']), evs:{spe:1}, ability:'Electromorphosis'}],
];
const poisonBarbFlingRosters = [
    [{...named('Pinsir', ['Fling', 'Recycle']), item:'Poison Barb', ability:'Mold Breaker'}],
    [named('Vaporeon', ['Recover']), {...named('Umbreon', ['Recover']), item:'Covert Cloak'}],
];
const berryFlingRosters = [
    [{...named('Farigiraf', ['Fling', 'Recover']), item:'Sitrus Berry', ability:'Cud Chew'}],
    [{...named('Greedent', ['Tackle', 'Recover']), ability:'Cheek Pouch'}],
];
const cudChewRosters = [
    [{...named('Farigiraf', ['Tackle']), ability:'Cud Chew', item:'Sitrus Berry'}],
    [named('Raticate', ['Super Fang', 'Recover'])],
];
const residualBerryRosters = [
    cudChewRosters[0],
    [named('Gengar', ['Toxic', 'Recover'])],
];
const megaSolRosters = [
    [{...named('Meganium', ['Solar Beam', 'Synthesis', 'Weather Ball']), ability:'Overgrow', item:'Meganiumite'}],
    [{...named('Milotic', ['Ice Beam', 'Recover']), ability:'Drizzle', item:'Utility Umbrella'}],
];
const assuranceRosters = [
    [named('Jolteon', ['Tackle']), named('Umbreon', ['Assurance'])],
    [named('Vaporeon', ['Recover']), named('Leafeon', ['Recover'])],
];
const clearSmogRosters = [
    [named('Gastrodon', ['Clear Smog'])],
    [named('Hatterene', ['Calm Mind', 'Recover'])],
];
const infernalParadeRosters = [
    [named('Typhlosion-Hisui', ['Will-O-Wisp', 'Infernal Parade'])],
    [named('Umbreon', ['Recover'])],
];
const chargeAfterDropsRosters = [
    [named('Luxray', ['Superpower'])],
    [{...named('Bellibolt', ['Recover']), ability:'Electromorphosis'}],
];
const healBellRosters = [
    [named('Snorlax', ['Recover']), named('Blissey', ['Heal Bell'])],
    [named('Gengar', ['Toxic', 'Recover'])],
];
const soundproofHealBellRosters = [
    [named('Blissey', ['Heal Bell', 'Recover']), {...named('Electrode', ['Recover']), ability:'Soundproof'}],
    [named('Gengar', ['Toxic', 'Recover']), named('Vaporeon', ['Recover'])],
];
const swaggerRosters = [
    [{...named('Sableye', ['Swagger']), ability:'Prankster', item:'Wide Lens'}],
    [named('Vaporeon', ['Recover'])],
];
const magicPowderRosters = [
    [named('Hatterene', ['Magic Powder'])],
    [named('Vaporeon', ['Recover'])],
];
const ingrainRosters = [
    [{...named('Charizard', ['Ingrain', 'Aqua Ring', 'Recover']), item:'Big Root'}, named('Vaporeon', ['Recover'])],
    [named('Diglett', ['Earthquake', 'Roar', 'Mud-Slap'])],
];
const strengthSapRosters = [
    [{...named('Vileplume', ['Swords Dance', 'Strength Sap']), item:'Big Root'}],
    [named('Metapod', ['Night Shade'])],
];
const gravityRosters = [
    [named('Flapple', ['Gravity', 'Grav Apple', 'Earthquake', 'Recover'])],
    [named('Charizard', ['Fly', 'Recover'])],
];
const steelRollerRosters = [
    [named('Copperajah', ['Steel Roller'])],
    [named('Leafeon', ['Grassy Terrain', 'Recover'])],
];
const traceSwitchRosters = [
    [{...named('Pinsir', ['Recover']), ability:'Mold Breaker'}],
    [named('Vaporeon', ['Recover']), {...named('Gardevoir', ['Recover']), ability:'Trace'}],
];
const intimidateSubstituteRosters = [
    [named('Vaporeon', ['Recover']), {...named('Wyrdeer', ['Recover']), ability:'Intimidate'}],
    [named('Typhlosion', ['Substitute', 'Recover'])],
];
const direClawSleepRosters = [
    [named('Sneasler', ['Dire Claw'])],
    [named('Avalugg', ['Recover'])],
];
const fellStingerRosters = [
    [named('Beedrill', ['Fell Stinger'])],
    [named('Abra', ['Calm Mind']), named('Vaporeon', ['Recover'])],
];
const statChangeRosters = [
    [named('Bronzong', ['Power Split', 'Power Swap']), named('Vaporeon', ['Recover'])],
    [named('Machamp', ['Swords Dance', 'Recover', 'Low Kick'])],
];
const wonderRoomRosters = [
    [named('Steelix', ['Wonder Room', 'Amnesia', 'Body Press'])],
    [named('Vaporeon', ['Calm Mind', 'Recover'])],
];
const parentalBondRosters = [
    [{...named('Kangaskhan', ['Power-Up Punch', 'Body Slam']), item:'Kangaskhanite'}],
    [named('Vaporeon', ['Recover'])],
];
const parentalSpreadRosters = [
    [{...named('Kangaskhan', ['Earthquake']), item:'Kangaskhanite'}, named('Umbreon', ['Recover'])],
    [named('Vaporeon', ['Recover']), named('Leafeon', ['Recover'])],
];
const disableFirstMoveRosters = [
    [named('Glalie', ['Disable'])],
    [named('Torkoal', ['Tackle'])],
];
const sweetVeilRosters = [
    [{...named('Ribombee', ['Recover']), ability:'Sweet Veil'}, named('Vaporeon', ['Recover'])],
    [named('Amoonguss', ['Spore']), named('Gengar', ['Yawn'])],
];
const guardDogRosters = [
    [{...named('Mabosstiff', ['Recover']), ability:'Guard Dog'}, named('Vaporeon', ['Recover'])],
    [{...named('Arcanine', ['Roar']), ability:'Intimidate'}],
];
const syrupRosters = [
    [{...named('Dipplin', ['Recover']), ability:'Supersweet Syrup'}, named('Vaporeon', ['Recover'])],
    [named('Umbreon', ['Recover'])],
];
const faintedItemSpeedRosters = [
    [named('Flareon', ['Healing Wish']), named('Vaporeon', ['Recover']), named('Glaceon', ['Recover'])],
    [{...named('Clefable', ['Healing Wish']), item:'Choice Scarf'},
        named('Umbreon', ['Recover']), named('Espeon', ['Recover'])],
];
const orbRosters = [
    [{...named('Gliscor', ['Recover']), ability:'Poison Heal', item:'Toxic Orb'},
        {...named('Machamp', ['Tackle']), ability:'Guts', item:'Flame Orb'}],
    [named('Umbreon', ['Tackle', 'Recover']), named('Vaporeon', ['Recover'])],
];
const wideLensRosters = [
    [{...named('Jolteon', ['Screech']), item:'Wide Lens'}, named('Vaporeon', ['Recover'])],
    [{...named('Froslass', ['Recover', 'Snowscape']), ability:'Snow Cloak'}, named('Umbreon', ['Recover'])],
];
const helpingHandRosters = [
    [named('Jolteon', ['Tackle', 'Helping Hand']), named('Slowbro', ['Helping Hand', 'Recover']),
        named('Vaporeon', ['Recover'])],
    [named('Umbreon', ['Recover']), named('Espeon', ['Recover'])],
];
const pranksterRosters = [
    [{...named('Sableye', ['Will-O-Wisp', 'Recover']), ability:'Prankster'}],
    [named('Umbreon', ['Recover']), named('Espeon', ['Tail Whip', 'Psychic Terrain'])],
];
const screenBreakingRosters = [
    [named('Umbreon', ['Psychic Fangs', 'Recover'])],
    [named('Espeon', ['Reflect', 'Substitute', 'Recover'])],
];
const toxicDebrisRosters = [
    [named('Vaporeon', ['Water Gun', 'Tackle']), named('Flareon', ['Recover'])],
    [{...named('Glimmora', ['Recover']), ability:'Toxic Debris'}],
];
const liquidOozeRosters = [
    [named('Vaporeon', ['Giga Drain', 'Leech Seed', 'Recover']),
        {...named('Clefable', ['Giga Drain']), ability:'Magic Guard'}],
    [{...named('Swalot', ['Recover']), ability:'Liquid Ooze'}],
];
const compoundEyesRosters = [
    [{...named('Froslass', ['Sing', 'Recover']), ability:'Compound Eyes', item:'Wide Lens'}],
    [{...named('Froslass', ['Snowscape', 'Recover']), ability:'Snow Cloak'}],
];
const faintBeforePivotRosters = [
    [named('Abra', ['Recover']), {...named('Abra', ['Recover']), name:'Abra II'},
        named('Vaporeon', ['Recover'])],
    [{...named('Perrserker', ['Swords Dance', 'U-turn']), evs:{spe:32, atk:32}},
        named('Froslass', ['Icy Wind', 'Recover']), named('Umbreon', ['Recover'])],
];
const allyDamageGuardRosters = [
    [{...named('Haxorus', ['Tackle', 'Recover']), ability:'Mold Breaker'},
        named('Vaporeon', ['Surf', 'Recover'])],
    [named('Umbreon', ['Recover', 'Protect']),
        {...named('Clefairy', ['Recover']), ability:'Friend Guard'}],
];
const priorityGuardRosters = [
    [named('Jolteon', ['Quick Attack', 'Recover']),
        {...named('Haxorus', ['Quick Attack', 'Recover']), ability:'Mold Breaker'}],
    [{...named('Tsareena', ['Recover']), ability:'Queenly Majesty'},
        {...named('Farigiraf', ['Recover']), ability:'Armor Tail'}],
];
const feintRosters = [
    [named('Flareon', ['Feint']), named('Vaporeon', ['Tackle'])],
    [named('Umbreon', ['Protect', 'Recover']), named('Espeon', ['Recover'])],
];
const sheerForceRosters = [
    [{...named('Nidoking', ['Ice Beam', 'Tackle', 'Stone Axe', 'Electro Shot'], 'M'),
        ability:'Sheer Force', item:'Life Orb'}],
    [named('Umbreon', ['Recover', 'Rain Dance', 'Substitute'])],
];
const facadeRosters = [
    [{...named('Umbreon', ['Facade', 'Tackle']), item:'Flame Orb'}],
    [named('Vaporeon', ['Recover'])],
];
const harvestRosters = [
    [{...named('Tropius', ['Recover', 'Sunny Day']), ability:'Harvest', item:'Sitrus Berry'}],
    [named('Leafeon', ['Super Fang', 'Recover'])],
];
const pickupRosters = [
    [{...named('Dedenne', ['Recover']), ability:'Pickup'},
        {...named('Umbreon', ['Super Fang', 'Recover']), item:'Sitrus Berry'}],
    [named('Espeon', ['Super Fang', 'Recover']),
        {...named('Vaporeon', ['Recover']), item:'Sitrus Berry'}],
];
const berserkRosters = [
    [{...named('Flareon', ['Seismic Toss', 'Double Kick']), item:'Life Orb'}],
    [{...named('Drampa', ['Calm Mind', 'Recover']), ability:'Berserk', item:'Sitrus Berry'}],
];
const suppressedBerserkRosters = [
    [{...named('Nidoking', ['Ice Beam', 'Sandstorm'], 'M'), ability:'Sheer Force'}],
    [{...named('Drampa', ['Calm Mind']), ability:'Berserk', item:'Sitrus Berry'}],
];
const angerShellRosters = [
    [named('Flareon', ['Seismic Toss'])],
    [{...named('Klawf', ['Swords Dance']), ability:'Anger Shell', item:'Sitrus Berry'}],
];
const noRetreatRosters = [
    [named('Falinks', ['No Retreat', 'Tackle'], 'N'), named('Vaporeon', ['Recover'])],
    [named('Umbreon', ['Recover'])],
];
const ghostNoRetreatRosters = [
    [named('Froslass', ['No Retreat'], 'F'), named('Vaporeon', ['Recover'])],
    [named('Umbreon', ['Recover'])],
];
const analyticRosters = [
    [{...named('Vaporeon', ['Tackle', 'Quick Attack']), ability:'Analytic'}],
    [named('Jolteon', ['Tail Whip']), named('Espeon', ['Recover'])],
];
const galeWingsRosters = [
    [{...named('Talonflame', ['Wing Attack']), ability:'Gale Wings'}],
    [named('Jolteon', ['Tackle'])],
];
const airborneRosters = [
    [named('Gyarados', ['Bounce', 'Fly'])],
    [named('Vaporeon', ['Gust', 'Tackle', 'Recover'])],
];
const rivalryRosters = [
    [{...named('Haxorus', ['Tackle'], 'M'), ability:'Rivalry'}],
    [named('Umbreon', ['Recover'], 'M'), named('Vaporeon', ['Recover'], 'F'),
        named('Metagross', ['Recover'], 'N')],
];
const cosmeticRosters = [
    [{...named('Furfrou-Heart', ['Tackle']), name:'Furfrou', ability:'Fur Coat'},
        named('Vivillon-Polar', ['Gust'])],
    [named('Umbreon', ['Recover'])],
];
const perishRosters = [
    [named('Jolteon', ['Perish Song', 'Recover'])],
    [named('Umbreon', ['Recover'])],
];
const perishDoublesRosters = [
    [named('Jolteon', ['Perish Song', 'Recover']), named('Vaporeon', ['Recover']),
        named('Flareon', ['Recover'])],
    [{...named('Exploud', ['Recover']), ability:'Soundproof'}, named('Umbreon', ['Recover']),
        named('Espeon', ['Recover'])],
];
const hustleZoomRosters = [
    [{...named('Durant', ['Tackle', 'Quick Attack', 'Water Gun']), ability:'Hustle', item:'Zoom Lens'}],
    [named('Espeon', ['Recover']), named('Umbreon', ['Recover'])],
];
const fireManeRosters = [
    [{...named('Pyroar', ['Flame Charge', 'Flamethrower', 'Tackle']),
        ability:'Unnerve', item:'Pyroarite'}],
    [named('Vaporeon', ['Recover'])],
];
const flyingPressRosters = [
    [named('Hawlucha', ['Flying Press'])],
    [named('Umbreon', ['Recover']), named('Froslass', ['Recover']), named('Vileplume', ['Recover'])],
];
const effectSporeRosters = [
    [{...named('Magikarp', ['Tackle']), item:'Lum Berry'}, named('Leafeon', ['Tackle'])],
    [{...named('Amoonguss', ['Recover']), ability:'Effect Spore'}],
];
const electricChargeRosters = [
    [{...named('Bellibolt', ['Charge', 'Thunder Shock', 'Water Gun', 'Recover']),
        ability:'Electromorphosis'}],
    [named('Umbreon', ['Tackle', 'Protect', 'Recover'])],
];
const windPowerRosters = [
    [named('Jolteon', ['Tailwind', 'Recover']),
        {...named('Kilowattrel', ['Thunder Shock', 'Recover']), ability:'Wind Power'}],
    [named('Umbreon', ['Gust', 'Recover']), named('Vaporeon', ['Recover'])],
];
const palafinRosters = [
    [{...named('Palafin', ['Flip Turn', 'Tackle']), ability:'Zero to Hero'},
        named('Vaporeon', ['Recover'])],
    [named('Umbreon', ['Recover'])],
];
const throatChopRosters = [
    [named('Umbreon', ['Throat Chop', 'Tackle', 'Recover'])],
    [named('Sylveon', ['Hyper Voice', 'Recover']),
        {...named('Slowbro', ['Hyper Voice', 'Recover']), item:'Covert Cloak'}],
];
const klutzRosters = [
    [{...named('Lopunny', ['Tackle', 'Quick Attack']), ability:'Klutz', item:'Choice Band'},
        {...named('Bewear', ['Recover']), ability:'Klutz', item:'Air Balloon'},
        {...named('Audino', ['Tackle']), ability:'Klutz', item:'Sitrus Berry'}],
    [named('Umbreon', ['Tackle', 'Earthquake', 'Knock Off', 'Seismic Toss'])],
];
const tantrumRosters = [
    [named('Arcanine', ['Recover', 'Stomping Tantrum', 'Tackle'])],
    [named('Umbreon', ['Recover', 'Protect'])],
];
const lashOutRosters = [
    [named('Umbreon', ['Lash Out'])],
    [named('Jolteon', ['Tail Whip', 'Recover']),
        {...named('Arcanine', ['Recover']), ability:'Intimidate'}],
];
const ironBallRosters = [
    [named('Umbreon', ['Earthquake'])],
    [{...named('Zapdos', ['Recover'], 'N'), item:'Iron Ball'},
        {...named('Rotom', ['Recover'], 'N'), item:'Iron Ball', ability:'Levitate'},
        named('Mandibuzz', ['Recover'])],
];
const reversalRosters = [
    [named('Machamp', ['Reversal', 'Flail'])],
    [named('Vaporeon', ['Seismic Toss', 'Recover'])],
];
const paybackRosters = [
    [named('Umbreon', ['Payback'])],
    [named('Espeon', ['Recover']), named('Shuckle', ['Recover', 'Quick Attack'])],
];
const stealBerryRosters = [
    [{...named('Scolipede', ['Bug Bite', 'Belch', 'Recycle', 'Pluck']), ability:'Ripen'}],
    [{...named('Vaporeon', ['Tackle', 'Recover']), item:'Sitrus Berry'},
        {...named('Umbreon', ['Recover']), item:'Sitrus Berry', ability:'Sticky Hold'},
        {...named('Leafeon', ['Recover']), item:'Liechi Berry'}],
];
const beatUpRosters = [
    [named('Umbreon', ['Beat Up']), named('Machamp', ['Recover']), named('Scizor', ['Recover'])],
    [named('Mandibuzz', ['Recover', 'Will-O-Wisp'])],
];
const beatUpMoxieRosters = [
    [{...named('Scrafty', ['Beat Up']), ability:'Moxie', evs:{atk:32}},
        named('Machamp', ['Recover']), named('Scizor', ['Recover'])],
    [named('Abra', ['Calm Mind']), named('Vaporeon', ['Recover'])],
];
const battleBondRosters = [
    [{...named('Greninja-Bond', ['Water Gun', 'Water Shuriken'], 'M'), ability:'Battle Bond'}],
    [named('Geodude', ['Tackle']), named('Rhydon', ['Recover']), named('Vaporeon', ['Recover'])],
];
const ashGreninjaRosters = [
    [{...named('Greninja-Ash', ['Water Shuriken'], 'M'), ability:'Battle Bond'}],
    [named('Umbreon', ['Recover'])],
];
const addedTypeRosters = [
    [named('Gourgeist', ['Trick-or-Treat', "Forest's Curse", 'Tackle'])],
    [named('Umbreon', ['Recover'])],
];
const curseGlitchRosters = [
    [named('Jolteon', ['Trick-or-Treat']), named('Umbreon', ['Curse'])],
    [named('Flareon', ['Recover']), named('Glaceon', ['Recover'])],
];
const uproarRosters = [
    [named('Jolteon', ['Uproar', 'Water Gun', 'Recover'])],
    [named('Umbreon', ['Rest', 'Spore', 'Recover'])],
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
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1']],
        sunnyFreezeRosters, 'bss-sun-blocks-freeze', '1,2,3,13'],
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
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1'], ['move 2','move 2']],
        sameChoiceItemRosters, 'bss-trick-same-choice-item'],
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
    ['draft', [['team 12', 'team 123'], ['move 2', 'move 1'], ['move 1', 'switch 2'],
        ['move 1', 'switch 3'], ['move 3', 'move 1'], ['move 4', 'move 1']], sleepClauseRosters, 'draft-sleep-clause'],
    ['draft', [['team 12', 'team 123'], ['move 1', 'switch 2'], ['move 2', 'switch 3'],
        ['move 3', 'move 1'], ['move 1', 'move 1']], sleepTerrainRosters, 'draft-sleep-terrain'],
    ['vgc2026regmc', [['>show-openteamsheets'], ['team 1234', 'team 1234'], ['>show-openteamsheets']],
        openSheetRosters, 'vgc-open-sheets'],
    ['vgc2026regmcbo3', [['team 1234', 'team 1234']], openSheetRosters, 'vgc-forced-open-sheets'],
    ['bssregmc', [['team 12', 'team 12'], ['move 1', 'move 1'], ['switch 2', 'move 1'],
        ['move 1', 'move 1'], ['move 1', 'move 1']], wishRosters, 'bss-wish'],
    ['vgc2026regmc', [['team 12', 'team 12'], ['move 1, move 1 1', 'move 1 2, move 1'],
        ['move 2, move 1 1', 'move 1 2, move 2'], ['move 2, move 1 1', 'move 1 2, move 1'],
        ['move 2, move 1 1', 'move 1 2, move 1']], tailwindRosters, 'vgc-tailwind'],
    ['bssregmc', [['team 123', 'team 12'], ['move 1', 'move 1'], ['move 1', 'move 2'], ['switch 2', 'move 1'],
        ['switch 2', null], ['move 2', 'move 2'], ['move 1', 'move 2']], hitCountPowerRosters, 'bss-hit-count-power'],
    ['bssregmc', [['team 12', 'team 12'], ['move 2', 'move 1'], ['move 1', 'move 2'],
        ['switch 2', null], ['move 1', 'move 1'], ['move 1', 'move 3'],
        ['move 1', 'move 3'], ['switch 2', 'move 4']], switchAbilityRosters, 'bss-switch-abilities'],
    ['vgc2026regmc', [['team 12', 'team 12'], ['move 2 1, move 2 2', 'move 1, move 1'],
        ['move 1 1, move 1 2', 'move 1, move 1'], ['move 1 1, move 1 2', 'move 1, move 1'],
        ['move 1 1, move 1 2', 'move 1, move 1'], ['move 1 1, move 1 2', 'move 1, move 1']],
        contactStatusRosters, 'vgc-contact-status'],
    ['rosterranchrumble', [['team 1,2,5,10', 'team 4,8,9,3'], ['move 1 1, move 2', 'move 1 2, move 4 2']],
        ['0,0,0,0', '4,3,2,1'], 'ranch-start'],
    ['vgc2026regmc', [['team 123', 'team 12'], ['move 2, move 2', 'move 2 1, move 1'],
        ['move 1 1, move 3', 'move 3 2, move 2'], ['move 1 -2, move 4 1', 'move 4 1, move 2'],
        ['switch 3, move 4 1', 'move 1 1, move 2']], flashDryRosters, 'vgc-flash-dry-frisk'],
    ['bssregmc', [['team 12','team 12'], ['move 2','move 2'], ['move 3','move 1'], ['move 1','move 1']],
        keenRecklessRosters, 'bss-keen-reckless'],
    ['bssregmc', [['team 12','team 12'], ['move 2','move 1'], ['move 3','move 1'], ['move 1','move 2'],
        ['switch 2','move 1'], ['move 1','move 3'], ['switch 2','move 3']], purifyingRosters, 'bss-purifying-salt'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 1'], ['switch 2','move 2'], ['move 1','move 1'],
        ['move 2','switch 2'], ['move 1','switch 2']], disguiseRosters, 'bss-disguise'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 1']], disguiseMultihitRosters, 'bss-disguise-multihit'],
    ['bssregmc', [['team 12','team 12'], ['move 2','move 1'], ['move 2','move 2'], ['move 3','move 3'],
        ['move 2','move 4'], ['move 1','switch 2'], ['move 1','switch 2']], spinRosters, 'bss-rapid-spin'],
    ['bssregmc', [['team 12','team 123'], ['move 1','move 1'], ['move 2','switch 2'], ['move 1','move 1']],
        statusDragRosters, 'bss-status-drag'],
    ['vgc2026regmc', [['team 123','team 123'], ['move 1, move 1 1','move 1, move 1'],
        ['move 3, move 2','switch 3, move 1'], ['move 1, move 1 1','move 1, move 1']],
        dampRosters, 'vgc-damp'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 1']], aftermathRosters, 'bss-aftermath'],
    ['vgc2026regmc', [['team 12','team 123'], ['move 1 1, move 1','move 1, move 1']],
        dampAftermathRosters, 'vgc-damp-aftermath'],
    ['bssregmc', [['team 12','team 12'], ['move 2','move 1'], ['move 1','move 1']], chestoRestRosters, 'bss-chesto-rest'],
    ['randombattle', [['move 1','move 1'], ['move 1','move 1']], randomLevelRosters, 'random-native-supplied'],
    ['randomdoublesbattle', [['move 1 1, move 1', 'move 1 1, move 1']], randomLevelRosters, 'random-doubles-native-supplied'],
    ['randombattle', [['move 1','move 4'], ['move 4','move 1']],
        ['1,2,3,4', '50219,19703,3172,32117'], 'random-native-generated'],
    ['randomdoublesbattle', [['move 3, move 3', 'move 3, move 1 1']],
        ['50219,19703,3172,32117', '702,13344,30570,22958'], 'random-doubles-native-generated'],
    ['natdexdraft', [['team 12','team 123'], ['move 2','move 1'], ['move 1','switch 2'], ['move 1','switch 3']],
        sleepClauseRosters, 'natdex-draft-native'],
    ['bssregmc', [['team 12','team 123'], ['move 1','move 1'], ['move 1','move 2'],
        ['move 3','switch 2'], ['move 1','switch 3'], ['move 1','switch 2'], ['move 2','move 2']],
        soakRosters, 'bss-soak-type'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 1'], ['move 1','move 2']],
        ragingBullRosters, 'bss-raging-bull-furcoat'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 1'], ['move 2','move 1'], ['switch 2',null]],
        corrosionRosters, 'bss-corrosion-defiant'],
    ['bssregmc', [['team 12','team 12'], ['move 2','move 1'], ['move 1','move 2'], ['move 1','move 1']],
        contraryRosters, 'bss-contrary'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 2'], ['move 1','move 1'], ['move 2','move 1'],
        ['move 2','switch 2']], critRosters, 'bss-crit-ratio'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 1'], ['move 2','move 1']],
        mercilessRosters, 'bss-merciless'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 1'], ['move 1','move 2'],
        ['move 1','move 3'], ['move 1','move 4']], focusPunchRosters, 'bss-focus-punch'],
    ['vgc2026regmc', [['team 12','team 12'], ['move 1 1, move 1 1','move 1 2, move 1']],
        doublesFocusPunchRosters, 'vgc-focus-punch'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 1'], ['switch 2',null],
        ['move 1','move 1'], ['switch',null], ['switch 2','move 1']], reviveBenchRosters, 'bss-revive-bench'],
    ['bssregmc', [['team 21','team 12'], ['move 1','move 1']], reviveBenchRosters, 'bss-revive-fails'],
    ['vgc2026regmc', [['team 12','team 12'], ['move 1 1, move 1','move 1 1, move 1'],
        ['pass, switch 1',null]], reviveActiveRosters, 'vgc-revive-active'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 1'], ['move 1','move 2'], ['switch 2',null],
        ['switch 2','move 2'], ['move 1','move 2'], ['switch 2',null]], chillyRosters, 'bss-chilly-reception'],
    ['vgc2026regmc', [['team 12','team 12'], ['move 1, move 1','move 2 1, move 1'],
        ['move 1, move 1','move 2 1, move 1']], chillyRosters, 'vgc-chilly-reception'],
    ['bssregmc', [['team 12','team 12'], ...Array(5).fill(['move 1','move 1'])], fickleBeamRosters, 'bss-fickle-beam'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 2'], ['move 3','move 2'], ['move 1','move 1'],
        ['move 2','move 3'], ['move 4','move 3']], statusPowerRosters, 'bss-status-power-haze'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 1'], ['switch 2','switch 2'], ['move 1','move 1']],
        syncRosters, 'bss-synchronize'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 1'], ['move 1','switch 2'], ['move 1','move 1']],
        syncLimberRosters, 'bss-sync-limber'],
    ['vgc2026regmc', [['team 12','team 12'], ['move 2, move 1','move 1 1, move 1'],
        ['move 1, move 1','move 2, move 2'], ['move 1, move 1','move 1 2, move 3']],
        mortalSpinRosters, 'vgc-mortal-spin'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 1'], ['move 1','move 2'], ['move 2','move 3'],
        ...Array(3).fill(['move 2','move 2'])], substituteRosters, 'bss-substitute'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 1'], ['switch 2',null], ['move 1','move 1'],
        ['move 1','move 3']], shedTailRosters, 'bss-shed-tail'],
    ['vgc2026regmc', [['team 12','team 12'], ['move 1, move 1','move 1, move 1']],
        mixedShieldRosters, 'vgc-substitute-spread'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 1'], ['move 2','move 1']], shieldFocusRosters, 'bss-substitute-focus'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 1'], ['move 1','switch 2'], ['move 2','move 1']],
        multiShieldRosters, 'bss-substitute-multihit'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 1'], ['move 2','move 2'], ['move 1','move 3']],
        shieldSecondariesRosters, 'bss-substitute-secondaries'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 1'], ['move 1','move 2'], ['move 1','move 3'],
        ['switch 2','move 3'], ['switch 2','move 3'], ['move 1','move 3']],
        doubleShockRosters, 'bss-double-shock'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 1'], ['move 1','move 2']],
        burnUpRosters, 'bss-burn-up'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 1'], ['switch 2','switch 2']],
        exitRosters, 'bss-wimp-out-double-pivot'],
    ['bssregmc', [['team 12','team 12'], ['move 2','move 1']],
        exitBerryRosters, 'bss-emergency-exit-berry'],
    ['vgc2026regmc', [['team 12','team 12'], ['move 1, move 1 2','move 1, move 1 2'],
        ['move 2, move 2','move 1, move 2']], sparklingAriaRosters, 'vgc-sparkling-aria-liquid-voice'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 1'], ['move 1','move 2'], ['switch 2','move 3']],
        waterBubbleRosters, 'bss-water-bubble-bulletproof'],
    ['bssregmc', [['team 12','team 12'], ['>reseed 5678,222,111,4096',null], ['move 1','move 1'],
        ['>tiebreak',null]], doubleShockRosters, 'bss-reseed-tiebreak-fraction'],
    ['bssregmc', [['team 12','team 12'], ['>tiebreak',null]], burnUpRosters, 'bss-tiebreak-raw-hp'],
    ['bssregmc', [['team 12','team 12'], ['>tiebreak',null]], [burnUpRosters[0],burnUpRosters[0]], 'bss-tiebreak-tie'],
    ['bssregmc', [['>forcewin p2',null]], burnUpRosters, 'bss-forcewin'],
    ['bssregmc', [['team 12','team 12'], ['move 1','>forcelose p2']], burnUpRosters, 'bss-forcelose'],
    ['bssregmc', [['team 12','team 12'], ['>forcetie',null]], burnUpRosters, 'bss-forcetie'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 1'], ['move 2','move 2'], ['move 1','move 2'],
        ['move 2','move 1'], ['move 1','move 3']], counterRosters, 'bss-typed-counter'],
    ['vgc2026regmc', [['team 12','team 12'], ['move 1, move 1','move 1 1, move 1 1'],
        ['move 2, move 1','move 1 1, move 1 1']], doublesCounterRosters, 'vgc-typed-counter'],
    ['vgc2026regmc', [['team 12','team 123'], ['move 1, move 1 1','move 1 1, move 1']],
        faintedCounterRosters, 'vgc-counter-fainted-attacker'],
    ['bssregmc', [['team 12','team 12'], ...Array(5).fill(['move 1','move 1'])], soulRosters, 'bss-clangorous-soul'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 1'], ['move 1','move 2'], ['switch 2',null]],
        steelBeamRosters, 'bss-steel-beam'],
    ['bssregmc', [['team 12','team 12'], ...Array(5).fill(['move 1','move 1']), ['move 1','move 2']],
        quickClawRosters, 'bss-quick-claw'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 1'], ['move 2','move 1']], lightBallRosters, 'bss-light-ball'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 1'], ['move 2','move 2'], ['move 1','move 3']],
        speedAbilityRosters, 'bss-steadfast-surge-surfer'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 1'], ['move 2','move 2'], ['move 3','move 1'],
        ['move 1','move 1'], ['move 2','move 3'], ['move 1','move 1'], ['switch 2','move 1'], ['move 1','switch 2']],
        bindingRosters, 'bss-binding-move-removal'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 1'], ...Array(5).fill(['move 2','move 1'])],
        bindingExpiryRosters, 'bss-binding-move-expiry'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 1'], ['move 2','move 2'], ['switch 2','move 3']],
        screenCleanRosters, 'bss-screen-cleaner'],
    ['vgc2026regmc', [['team 12','team 12'], ['move 1 2, move 1','move 1, move 1 1'],
        ['move 2 1, move 1','move 1, move 1 1'], ['move 3 2, move 2 2','move 1, move 1 1']],
        aromaRosters, 'vgc-aroma-veil'],
    ['vgc2026regmc', [['team 12','team 12'], ...Array(4).fill(['move 1 2, move 1','move 1, move 1'])],
        healerRosters, 'vgc-champions-healer'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 1'], ['move 2','move 2'], ['move 2','move 3'],
        ['move 3','switch 2'], ['move 2','move 1'], ['move 2','move 2']], unawareRosters, 'bss-unaware'],
    ['vgc2026regmc', [['team 12','team 12'], ['move 1, move 1','move 1, move 1'],
        ['move 2 1, move 1','move 2 1, move 1']], doubleUnawareRosters, 'vgc-unaware-speed-tie'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 1'], ['move 2','move 2'], ['move 3','move 3'],
        ['switch 2',null], ['move 1','move 2']], batonShieldRosters, 'bss-baton-pass-boosts-and-shield'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 1'], ['move 1','move 4'], ['move 1','move 2'],
        ['move 2','move 3'], ['switch 2',null]], batonBoundRosters, 'bss-baton-pass-conditions'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 1'], ['move 1','move 1'], ['move 1','switch 2'],
        ['move 1','move 1'], ['move 1','switch 2']], rainDishRosters, 'bss-rain-dish'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 1'], ['move 1','move 1'], ['move 1','move 1'],
        ['move 1','switch 2'], ['move 1','move 1']], snowCloakRosters, 'bss-snow-cloak'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 1'], ['move 3','move 2'], ['move 3','move 3'],
        ['move 2','move 4']], sacredSwordRosters, 'bss-sacred-sword'],
    ['vgc2026regmc', [['team 12','team 12'], ['move 2 2, move 1','move 1, move 1'],
        ['move 1 2, move 1','move 1, move 1']], snipeShotRosters, 'vgc-snipe-shot'],
    ['bssregmc', [['team 12','team 123'], ['move 1','move 1'], ['move 2','move 1'], ['move 1','switch 2'],
        ['move 2','move 1'], ['move 3','switch 3'], ['move 1','move 1'], ['move 2','move 1'],
        ['move 4','move 1'], ['move 1','move 1'], ['move 3','move 1']], yawnAbilityRosters, 'bss-yawn-abilities'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 3'], ['move 3','move 2'],
        ['move 3','move 1'], ['move 2','move 2'], ['move 3','move 2'], ['move 3','move 2']], earlyBirdRosters, 'bss-early-bird'],
    ['bssregmc', [['team 12','team 12'], ['move 1','switch 2'], ['move 2','move 1'], ['move 1','switch 2']],
        scrappyRosters, 'bss-scrappy'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 1'], ['move 2','move 1'], ['move 1','switch 2'],
        ['move 2','move 1']], secondaryProtectionRosters, 'bss-secondary-protection'],
    ['vgc2026regmc', [['team 123','team 12'], ['move 1, move 1','move 1, move 1'],
        ['move 2, move 1','move 1, move 1'], ['switch 3, move 1','move 1, move 1'],
        ['move 1, move 1','move 1, move 1']], telepathyRosters, 'vgc-telepathy'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 1'], ['move 1','move 2'],
        ['move 2','move 3'], ['move 2','switch 2'], ['move 2','move 1']], attackHazardRosters, 'bss-attack-hazards'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 1'], ['move 2','move 2'],
        ['move 1','move 3'], ['switch 2','move 3'], ['switch 2','move 3']],
        repeatedHammerRosters, 'bss-gigaton-hammer'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 1'], ['move 2','move 3'], ['move 1','move 1'],
        ['move 2','move 2'], ['move 1','move 2']],
        bloodMoonRosters, 'bss-blood-moon'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 1'], ['switch 2','move 2'],
        ['switch 2','move 4'], ...Array(4).fill(['move 1','move 4'])], mimicryRosters, 'bss-mimicry'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 1'], ...Array(3).fill(['move 2','move 1'])],
        angerPointRosters, 'bss-anger-point'],
    ['bssregmc', [['team 12','team 12'], ['move 2 mega','move 1'], ['move 1','move 2'],
        [null,'switch 2'], ['move 2','move 1']], eelevateRosters, 'bss-eelevate'],
    ['bssregmc', [['team 12','team 12'], ['move 3','move 1'], ['move 1','move 2'], ['move 3','move 3'],
        ['move 1','move 2'], ['switch 2','move 2'], ['switch 2','move 2'], ['move 2','move 2']],
        recycleRosters, 'bss-recycle-unburden'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 1'], ['move 2','move 2']],
        poppedBalloonRosters, 'bss-recycle-popped-balloon'],
    ['bssregmc', [['team 12','team 12'], ['move 3','move 1'], ['move 4','move 2'], ['move 1','move 3'],
        ['move 1','move 4'], ['move 1','switch 2']], defogRosters, 'bss-defog'],
    ['vgc2026regmc', [['team 123','team 12'], ['move 1 2, move 1','move 1 1, move 1'],
        ['move 1 2, switch 3','move 1 1, move 1'], ['move 1 2, move 1 -1','move 2, move 1'],
        ['move 2, move 1 -1','move 2, move 1'], ['move 2, move 1 2','move 2, move 1']],
        hospitalityRosters, 'vgc-hospitality-pollen-puff'],
    ['bssregmc', [['team 123','team 12'], ['move 1','move 1'], ['switch 2',null],
        ['move 1','move 1'], ['switch 3','move 1'], ['move 1','move 1'], ['switch 3',null],
        ['move 1','move 1']], overlordRosters, 'bss-supreme-overlord'],
    ['vgc2026regmc', [['team 123','team 12'], ['move 1, move 1','move 1, move 1'],
        ['pass, switch 3',null], ['move 2 1, move 1','move 1, move 1']],
        receiverRosters, 'vgc-receiver'],
    ['vgc2026regmc', [['team 123','team 12'], ['move 1, move 1','move 1, move 1']],
        receiverUnnerveRosters, 'vgc-receiver-unnerve'],
    ['bssregmc', [['team 123','team 12'], ['move 1','move 1'], ['switch 2','move 2'],
        ['move 1','move 2'], ['switch 3',null], ['move 1','move 3'], ['switch 2','move 2']],
        healingWishRosters, 'bss-healing-wish'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 1'], ['switch 2','move 1'],
        ['move 1','move 1'], ['switch 2',null], ['move 1','move 1']],
        lunarDanceRosters, 'bss-lunar-dance'],
    ['bssregmc', [['team 123','team 12'], ['move 1','move 1'], ['move 1','move 1'], ['switch 2','move 2'],
        ['move 1','move 2'], ['switch 3','move 3'], ['move 1','move 1']],
        ripenRosters, 'bss-ripen'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 1'], ['move 2','switch 2'],
        ['move 1','move 1'], ['move 2','move 2'], ['move 1','move 2']],
        beakBlastRosters, 'bss-beak-blast'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 1'], ['move 1','move 2'],
        ['move 2','switch 2'], ['move 1','move 1'], ['move 1','move 1']],
        curseRosters, 'bss-ghost-curse'],
    ['vgc2026regmc', [['team 12','team 12'], ['move 1, move 1 -1','move 1, move 1'],
        ['move 1, move 2','move 1, move 1']], curseDoublesRosters, 'vgc-curse-targets'],
    ['vgc2026regmc', [['team 123','team 12'], ['move 1, move 1','move 1, move 1'],
        ['move 2 2, move 1','move 1, move 1'], ['move 2 2, switch 3','move 1, move 1']],
        allyAbilitiesRosters, 'vgc-plus-minus-curious-medicine'],
    ['vgc2026regmc', [['team 12','team 12'], ['move 1 -2, move 1','move 1, move 1'],
        ['move 1 -1, move 1','move 1, move 1']], acupressureRosters, 'vgc-acupressure'],
    ['vgc2026regmc', [['team 12','team 12'], ['move 1, move 1','move 1, move 1']],
        magneticFluxRosters, 'vgc-magnetic-flux'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 1'], ['move 1','switch 2'],
        ['switch 2','move 2'], ['switch 2','move 1'], ['move 1','switch 2'],
        ['move 1','move 1'], ['move 1','move 1']], futureSightRosters, 'bss-future-sight'],
    ['vgc2026regmc', [['team 12','team 12'], ['move 1 1, move 1 2','move 1, move 1'],
        ['move 2, move 2','move 2, move 1'], ['move 2, move 2','move 2, move 1']],
        doubleFutureRosters, 'vgc-delayed-attacks'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 1'], ['switch 2',null],
        ['move 1','switch 2'], ['move 1','move 1']], faintedFutureRosters, 'bss-future-fainted-user'],
    ['bssregmc', [['team 12','team 12'], ['move 2','move 2'], ['move 1','move 1'], ['move 2','move 2'],
        ['switch 2','move 2'], ['switch 2','move 2'], ['move 2','move 2']], belchRosters, 'bss-belch'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 2'], ['switch 2','move 3'],
        ['switch 2','move 1'], ['move 1','move 3']], imposterRosters, 'bss-imposter'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 1'], ['move 2','move 3'],
        ['switch 2','move 3'], ['switch 2','move 3'], ['move 1','move 3']],
        transformRosters, 'bss-transform'],
    ['vgc2026regmc', [['team 12','team 12'], ['move 1 1, move 1','move 1, move 1 1']],
        imposterDoublesRosters, 'vgc-imposter'],
    ['vgc2026regmc', [['team 12','team 12'], ['move 2 1, move 1','move 1 1, move 1']],
        imposterFaintRosters, 'vgc-imposter-faints-before-moving'],
    ['bssregmc', [['team 12','team 123'], ['move 1','move 1'], ['move 1','move 2'], ['move 1','switch 3']],
        dartRosters, 'bss-dragon-darts'],
    ['vgc2026regmc', [['team 12','team 123'], ['move 1 2, move 1','move 1, move 1'],
        ['move 1 1, move 1','move 2, move 1'], ['move 1 2, move 1','move 3, move 1'], ['move 1 1, move 1','switch 3, move 1'],
        ['move 1 -2, move 1','move 1, move 1']], dartRosters, 'vgc-dragon-darts'],
    ['bssregmc', [['team 12','team 123'], ['move 1','move 1'], ['switch 2','move 1'],
        ['move 1','move 1'], ['move 2','switch 2'], ['move 2','switch 3']],
        superEffectiveGuardRosters, 'bss-super-effective-guards'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 1'], ['move 2','move 1'],
        ['move 3','switch 2'], ['move 3','move 1']], defenseHookRosters, 'bss-defense-hit-hooks'],
    ['vgc2026regmc', [['team 123','team 123'], ['move 1, move 1','move 1, move 1'],
        ['switch 3, pass','switch 3, pass']], faintedItemSpeedRosters, 'vgc-fainted-choice-scarf'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 1'], ['move 1','move 1'],
        ['switch 2','move 1'], ['move 1','move 2'], ['switch 2','move 1'], ['move 1','move 1']],
        orbRosters, 'bss-orbs-and-poison-heal'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 1'], ['move 1','move 2'],
        ...Array(4).fill(['move 1','move 1'])], wideLensRosters, 'bss-wide-lens'],
    ['vgc2026regmc', [['team 123','team 12'], ['move 1 1, move 1 -1','move 1, move 1'],
        ['move 1 1, move 2','move 1, move 1'], ['move 2 -2, move 1 -1','move 1, move 1'],
        ['switch 3, move 1 -1','move 1, move 1']], helpingHandRosters, 'vgc-helping-hand'],
    ['bssregmc', [['team 1','team 12'], ['move 1','move 1'], ['move 1','switch 2'],
        ['move 1','move 1'], ['move 2','move 2'], ['move 1','move 1']],
        pranksterRosters, 'bss-prankster'],
    ['bssregmc', [['team 1','team 1'], ['move 2','move 2'], ['move 1','move 1'], ['move 1','move 3']],
        screenBreakingRosters, 'bss-psychic-fangs'],
    ['bssregmc', [['team 12','team 1'], ['move 1','move 1'], ...Array(3).fill(['move 2','move 1']),
        ['switch 2','move 1']], toxicDebrisRosters, 'bss-toxic-debris'],
    ['bssregmc', [['team 12','team 1'], ['move 1','move 1'], ['move 2','move 1'], ['switch 2','move 1'],
        ['move 1','move 1']], liquidOozeRosters, 'bss-liquid-ooze'],
    ['bssregmc', [['team 1','team 1'], ['move 2','move 1'], ...Array(5).fill(['move 1','move 2'])],
        compoundEyesRosters, 'bss-compound-eyes'],
    ['vgc2026regmc', [['team 123','team 123'], ['move 1, move 1','move 1, move 1'],
        ['move 1, move 1','move 2 2, move 2'], [null,'switch 3, pass'], ['pass, switch 3',null]],
        faintBeforePivotRosters, 'vgc-faint-before-pivot'],
    ['vgc2026regmc', [['team 12','team 12'], ['move 1 1, move 1','move 1, move 1'],
        ['move 1 2, move 1','move 2, move 1']], allyDamageGuardRosters, 'vgc-friend-guard'],
    ['vgc2026regmc', [['team 12','team 12'], ['move 1 2, move 1 2','move 1, move 1'],
        ['move 1 -2, move 2','move 1, move 1']],
        priorityGuardRosters, 'vgc-priority-guards'],
    ['vgc2026regmc', [['team 12','team 12'], ['move 1 1, move 1 1','move 1, move 1'],
        ['move 1 1, move 1 1','move 2, move 1']], feintRosters, 'vgc-feint'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1'], ['move 2','move 2'], ['move 3','move 3'],
        ['move 4','move 1']], sheerForceRosters, 'bss-sheer-force'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1'], ['move 1','move 1'], ['move 2','move 1']],
        facadeRosters, 'bss-facade'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 2'], ['move 2','move 1'], ['move 1','move 1']],
        harvestRosters, 'bss-harvest'],
    ['vgc2026regmc', [['team 12','team 12'], ['move 1, move 1 2','move 1 2, move 1'],
        ['move 1, move 2','move 2, move 1']], pickupRosters, 'vgc-pickup'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1'], ['move 1','move 1'],
        ['move 2','move 2']], berserkRosters, 'bss-berserk'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1'], ['move 2','move 1']],
        suppressedBerserkRosters, 'bss-sheer-force-berserk'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1'], ['move 1','move 1']],
        angerShellRosters, 'bss-anger-shell'],
    ['bssregmc', [['team 12','team 1'], ['move 1','move 1'], ['move 1','move 1'], ['move 2','move 1']],
        noRetreatRosters, 'bss-no-retreat'],
    ['bssregmc', [['team 12','team 1'], ['move 1','move 1'], ['move 1','move 1'],
        ['switch 2','move 1']], ghostNoRetreatRosters, 'bss-ghost-no-retreat'],
    ['bssregmc', [['team 1','team 12'], ['move 1','move 1'], ['move 2','move 1'],
        ['move 1','switch 2']], analyticRosters, 'bss-analytic'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1'], ['move 1','move 1']],
        galeWingsRosters, 'bss-gale-wings'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1'], ['move 1','move 3'],
        ['move 2','move 1'], ['move 1','move 3'], ['move 1','move 2']],
        airborneRosters, 'bss-airborne-charge'],
    ['bssregmc', [['team 1','team 123'], ['move 1','move 1'], ['move 1','switch 2'],
        ['move 1','switch 3']], rivalryRosters, 'bss-rivalry'],
    ['bssregmc', [['team 12','team 1'], ['move 1','move 1'], ['switch 2','move 1'],
        ['move 1','move 1']], cosmeticRosters, 'bss-cosmetic-forms'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1'], ...Array(3).fill(['move 2','move 1'])],
        perishRosters, 'bss-perish-song-ending'],
    ['vgc2026regmc', [['team 123','team 123'], ['move 1, move 1','move 1, move 1'],
        ['switch 3, move 1','move 1, move 1'], ...Array(2).fill(['move 1, move 1','move 1, move 1']),
        ['pass, switch 3','pass, switch 3']], perishDoublesRosters, 'vgc-perish-song'],
    ['bssregmc', [['team 1','team 12'], ['move 1','move 1'], ...Array(3).fill(['move 2','move 1']),
        ['move 3','switch 2'], ['move 1','move 1']], hustleZoomRosters, 'bss-hustle-zoom-lens'],
    ['bssregmc', [['team 1','team 1'], ['move 1 mega','move 1'], ['move 2','move 1'],
        ['move 3','move 1']], fireManeRosters, 'bss-fire-mane'],
    ['bssregmc', [['team 1','team 123'], ['move 1','move 1'], ['move 1','switch 2'],
        ['move 1','switch 3']], flyingPressRosters, 'bss-flying-press'],
    ['bssregmc', [['team 12','team 1'], ...Array(7).fill(['move 1','move 1']),
        ['switch 2','move 1'], ['move 1','move 1']], effectSporeRosters, 'bss-effect-spore'],
    ['bssregmc', [['team 12','team 1'], ...Array(6).fill(['move 1','move 1'])],
        [[{...effectSporeRosters[0][0], item:''}, effectSporeRosters[0][1]], effectSporeRosters[1]],
        'bss-effect-spore-sleep', '1,2,3,8'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1'], ['move 3','move 1'], ['move 2','move 1'],
        ['move 4','move 1'], ['move 2','move 2'], ['move 2','move 3']],
        electricChargeRosters, 'bss-electric-charge'],
    ['vgc2026regmc', [['team 12','team 12'], ['move 1, move 1 1','move 1 2, move 1'],
        ['move 2, move 1 1','move 2, move 1']], windPowerRosters, 'vgc-wind-power'],
    ['bssregmc', [['team 12','team 1'], ['move 1','move 1'], ['switch 2',null], ['switch 2','move 1'],
        ['move 2','move 1'], ['switch 2','move 1'], ['switch 2','move 1']],
        palafinRosters, 'bss-zero-to-hero'],
    ['bssregmc', [['team 1','team 12'], ['move 1','move 1'], ['move 2','move 2'], ['move 2','move 1'],
        ['move 1','switch 2'], ['move 3','move 1']], throatChopRosters, 'bss-throat-chop'],
    ['bssregmc', [['team 123','team 1'], ['move 1','move 1'], ['move 2','move 1'], ['switch 2','move 2'],
        ['move 1','move 3'], ['switch 3','move 4'], ['move 1','move 4']],
        klutzRosters, 'bss-klutz'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1'], ['move 2','move 1'],
        ['move 3','move 2'], ['move 2','move 1']], tantrumRosters, 'bss-stomping-tantrum'],
    ['bssregmc', [['team 1','team 12'], ['move 1','move 1'], ['move 1','move 2'],
        ['move 1','switch 2']], lashOutRosters, 'bss-lash-out'],
    ['bssregmc', [['team 1','team 123'], ['move 1','move 1'], ['move 1','switch 2'],
        ['move 1','switch 3']], ironBallRosters, 'bss-iron-ball'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 2'], ['move 1','move 1'],
        ['move 2','move 1'], ['move 1','move 1']], reversalRosters, 'bss-reversal-flail'],
    ['bssregmc', [['team 1','team 12'], ['move 1','move 1'], ['move 1','switch 2'],
        ['move 1','move 1'], ['move 1','move 2']], paybackRosters, 'bss-payback'],
    ['bssregmc', [['team 1','team 123'], ['move 2','move 1'], ['move 1','move 2'], ['move 3','move 2'],
        ['move 2','move 2'], ['move 4','switch 2'], ['move 4','switch 3']],
        stealBerryRosters, 'bss-steal-berry'],
    ['bssregmc', [['team 123','team 1'], ['move 1','move 1'], ['switch 2','move 2'],
        ['switch 2','move 1'], ['move 1','move 1']], beatUpRosters, 'bss-beat-up'],
    ['bssregmc', [['team 123','team 12'], ['move 1','move 1']], beatUpMoxieRosters, 'bss-beat-up-moxie'],
    ['bssregmc', [['team 1','team 123'], ['move 1','move 1'], [null,'switch 2'], ['move 1','move 1'],
        [null,'switch 3'], ['move 2','move 1']], battleBondRosters, 'bss-battle-bond'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1']],
        ashGreninjaRosters, 'bss-ash-water-shuriken'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1'], ['move 3','move 1'],
        ['move 1','move 1'], ['move 2','move 1'], ['move 3','move 1']],
        addedTypeRosters, 'bss-added-types'],
    ['vgc2026regmc', [['team 12','team 12'], ['move 1 -2, move 1','move 1, move 1']],
        curseGlitchRosters, 'vgc-added-ghost-curse'],
    ['bssregmc', [['team 1','team 1'], ['move 2','move 1'], ['move 1','move 2'],
        ['move 1','move 3'], ['move 1','move 3'], ['move 3','move 2']],
        uproarRosters, 'bss-uproar'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1'], ['move 2','move 2'],
        ['move 3','move 2'], ['move 4','move 2']], specialPhysicalDefenseRosters, 'bss-special-physical-defense'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1'], ['move 2','move 1'],
        ['move 1','move 2'], ['move 2','move 2']], situationalPowerRosters, 'bss-situational-power'],
    ['bssregmc', [['team 1','team 12'], ['move 1','move 1'], ['move 2','move 1'],
        ['move 2','switch 2']], risingVoltageRosters, 'bss-rising-voltage'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1'], ['move 1','move 1']],
        contactSpeedRosters, 'bss-contact-stat-abilities'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1'], ['move 2','move 1']],
        stallRosters, 'bss-stall-priority'],
    ['bssregmc', [['team 1','team 1'], ['move 2','move 1'], ['move 1','move 2']],
        hydrationRosters, 'bss-hydration'],
    ['bssregmc', [['team 1','team 12'], ['move 1','move 1'], ['move 1','move 1'],
        ['move 3','move 1'], ['move 2','move 1'], ['move 1','switch 2']],
        confuseRayRosters, 'bss-confusion-accuracy'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1'], ['move 2','move 1'], ['move 3','move 1']],
        fluffyRosters, 'bss-fluffy'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1'], ['move 2','move 1'], ['move 1','move 2']],
        punkRockRosters, 'bss-punk-rock'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1 mega'], ['move 2','move 2']],
        spicySprayRosters, 'bss-spicy-spray'],
    ['bssregmc', [['team 12','team 1'], ['move 1','move 1'], ['move 1','move 1'],
        ['switch 2','move 1'], ['move 1','move 1']], bellyDrumRosters, 'bss-belly-drum'],
    ['vgc2026regmc', [['team 12','team 12'], ['move 1 1, move 1 1','move 1 2, move 1 2'],
        ['move 1 1, move 1 2','move 2 2, move 1 2']], quashRosters, 'vgc-quash'],
    ['bssregmc', [['team 12','team 123'], ['move 1','move 1'], ['move 1','switch 2'],
        ['switch 2','switch 3'], ['move 1','move 1']], theftRosters, 'bss-thief-covet'],
    ['bssregmc', [['team 12','team 1'], ['move 1','move 1'], ['move 2','move 1'],
        ['switch 2',null]], pickpocketRosters, 'bss-pickpocket'],
    ['bssregmc', [['team 12','team 1'], ['move 2','move 1'], ['switch 2',null]],
        pickpocketRosters, 'bss-pickpocket-pivot'],
    ['vgc2026regmc', [['team 12','team 12'], ['move 1, move 1 -1','move 2, move 2'],
        ['move 1, move 1 -1','move 1 1, move 1 2'], ['move 2 1, move 1 1','move 1 2, move 1 1'],
        ['move 1, move 1 -1','move 1 2, move 2']], teamHealingRosters, 'vgc-life-dew-heal-pulse'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 1'], ['move 1','move 2'], ['move 1','switch 2']],
        finalGambitRosters, 'bss-final-gambit'],
    ['bssregmc', [['team 12','team 12'], ['move 1 mega','move 1']],
        innardsOutRosters, 'bss-innards-out'],
    ['vgc2026regmc', [['team 12','team 12'], ['move 1, move 1','move 2, move 2']],
        dangerSenseRosters, 'vgc-entry-move-threats'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1'], ['move 1','move 1']],
        seedSowerRosters, 'bss-seed-sower'],
    ['bssregmc', [['team 1','team 12'], ['move 1','move 1'], ['move 2','switch 2'],
        ['move 3','switch 2'], ['move 4','move 1']], forecastRosters, 'bss-forecast'],
    ['bssregmc', [['team 12','team 1'], ['move 1','move 1'], ['move 1','move 1'],
        ['switch 2','move 1'], ['switch 2','move 1'], ['move 1','move 1']],
        hungerSwitchRosters, 'bss-hunger-switch'],
    ['vgc2026regmc', [['team 12','team 12'], ['move 1 1, move 1 1','move 1 1, move 1 1'],
        ['move 1, move 1 1','move 2, move 2']], phantomForceRosters, 'vgc-phantom-force'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1'], ['move 1','move 1'], ['move 1','move 1']],
        skyAttackRosters, 'bss-sky-attack'],
    ['bssregmc', [['team 1','team 12'], ['move 1','move 1'], ['move 1','switch 2'], ['move 1','move 1']],
        personalGuardsRosters, 'bss-personal-guards'],
    ['bssregmc', [['team 1','team 1'], ['move 1 mega','move 1'], ['move 1','move 2'], ['move 2','move 1']],
        unseenFistRosters, 'bss-unseen-fist'],
    ['vgc2026regmc', [['team 12','team 12'], ['move 1, move 1','move 1, move 1'],
        ['move 2, move 1','move 2 2, move 3'], ['move 1, move 1','move 1, move 2 2']],
        sideGuardRosters, 'vgc-side-guards'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1'], ['move 1','move 2'],
        ['move 2','move 2'], ['move 3','move 2']], retaliationPowerRosters, 'bss-retaliation-power'],
    ['vgc2026regmc', [['team 12','team 12'], ['move 1 -2, move 1 1','move 1, move 1'],
        ['move 2, move 1 1','move 1, move 1']], coachingRosters, 'vgc-coaching-burning-jealousy'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1']], endureRosters, 'bss-endure'],
    ['bssregmc', [['team 1','team 1'], ...Array(5).fill(['move 1','move 1']), ['move 2','move 1']],
        quickDrawRosters, 'bss-quick-draw'],
    ['bssregmc', [['team 1','team 1'], ...Array(5).fill(['move 1','move 1'])],
        poisonTouchRosters, 'bss-poison-touch'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1'], ['move 2','move 2'], ['move 2','move 1']],
        psychicNoiseRosters, 'bss-psychic-noise'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1']],
        [psychicNoiseRosters[0], [{...psychicNoiseRosters[1][0], item:'Mental Herb'}]], 'bss-psychic-noise-herb'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1'], ['move 2','move 2']],
        speedPowerRosters, 'bss-speed-power'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1'], ['move 2','move 2'], ['move 2','move 1']],
        terrainPulseRosters, 'bss-terrain-pulse-aqua-ring'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1'], ['move 1','move 2']],
        upperHandRosters, 'bss-upper-hand'],
    ['vgc2026regmc', [['team 12','team 12'], ['move 1 1, move 1 2','move 1, move 1']],
        steelySpiritRosters, 'vgc-steely-spirit'],
    ['vgc2026regmc', [['team 12','team 12'], ['move 1 2, move 1','move 1, move 1']],
        faintedMegaRosters, 'vgc-fainted-slot-mega-request'],
    ['vgc2026regmc', [['team 12','team 12'], ['move 1, move 1','move 1, move 1'],
        ['move 2 1, move 1','move 1, move 1']], fullMoonlightRosters, 'vgc-full-health-moonlight'],
    ['vgc2026regmc', [['team 12','team 123'], ['move 1 -2, move 1 1','move 1, move 1'],
        ['move 1 1, move 1 2','move 1, move 1'], ['move 1 2, move 1 1','move 1, switch 3']],
        instructRosters, 'vgc-instruct'],
    ['bssregmc', [['team 1','team 12'], ['move 1','move 1'], ['move 1','switch 2']],
        shellSideArmRosters, 'bss-shell-side-arm'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1'], ['move 1','move 2'],
        ['move 1','move 2'], ['move 1','move 2']], sleepTalkRosters, 'bss-sleep-talk'],
    ['bssregmc', [['team 1','team 1'], ['move 2','move 1'], ['move 1','move 1']],
        sleepTalkRestRosters, 'bss-sleep-talk-rest'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1'], ['move 2','move 2'], ['move 1','move 1']],
        tormentRosters, 'bss-torment'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 1'], ['move 2','move 1'],
        ['switch 2','move 1'], ['switch 2','switch 2'], ['move 1','move 1']],
        octolockRosters, 'bss-octolock'],
    ['bssregmc', [['team 1','team 12'], ['move 1','move 1'], ['move 2','switch 2'],
        ['move 2','move 1'], ['move 2','move 2'], ['move 2','move 2']], safeguardRosters, 'bss-safeguard'],
    ['bssregmc', [['team 1','team 12'], ['move 1','move 1'], ['move 2','move 1'], ['move 2','move 1'],
        ['move 3','move 1'], ['move 4','move 1'], ['move 4','move 1'], ['move 2','switch 2']],
        replaceAbilityRosters, 'bss-ability-replacement'],
    ['natdexdraft', [['team 12','team 1'], ['move 1','move 1'], ['move 2','move 1'],
        ['move hiddenpowerice60','move 1'], ['switch 2','move 1'], ['move 1','move 1']],
        legacyMoveRosters, 'natdex-legacy-moves'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1'], ['move 1','move 1']],
        cheekPouchRosters, 'bss-cheek-pouch'],
    ['bssregmc', [['team 12','team 1'], ['move 1','move 1'], ['switch 2','move 1'], ['move 1','move 1']],
        solarPowerRosters, 'bss-solar-power'],
    ['bssregmc', [['team 1','team 1'], ['move 1 mega','move 1'], ['move 1','move 2'], ['move 2','move 3']],
        piercingDrillRosters, 'bss-piercing-drill'],
    ['vgc2026regmc', [['team 12','team 123'], ['move 1 1, move 1','move 1, move 1'], ['move 1 1, move 1','move 1, switch 3']],
        auraRosters, 'vgc-auras'],
    ['vgc2026regmc', [['team 12','team 12'], ['move 1 2, move 1','move 1, move 1']], stalwartRosters, 'vgc-stalwart'],
    ['vgc2026regmc', [['team 12','team 123'], ['move 1 2, move 1','move 1, move 1']],
        stalwartHazardRosters, 'vgc-stalwart-side-target'],
    ['bssregmc', [['team 1','team 12'], ['move 1','move 1'], ['move 2','move 1'], ['move 1','switch 2'],
        ['move 1','move 1'], ['move 1','move 2']], stenchRosters, 'bss-stench', '1,2,3,13'],
    ['bssregmc', [['team 12','team 1'], ['move 1','move 2'], ['move 1','move 1']],
        redCardRosters, 'bss-red-card'],
    ['bssregmc', [['team 12','team 1'], ['move 1','move 1']],
        guardDogCardRosters, 'bss-red-card-guard-dog'],
    ['bssregmc', [['team 12','team 1'], ['move 1','move 1']],
        lifeOrbCardRosters, 'bss-red-card-life-orb'],
    ['bssregmc', [['team 1','team 1'], ['move 1 mega','move 1']],
        floettiteRosters, 'bss-floettite-knock-off'],
    ['bssregmc', [['team 1','team 12'], ['move 1','move 1'], ['move 2','move 1'],
        ['move 3','switch 2']], terrainSeedRosters, 'bss-terrain-seeds'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 1'], ['move 2','move 1'],
        ['switch 2',null], ['move 1','switch 2']], shackleRosters, 'bss-spirit-shackle'],
    ['bssregmc', [['team 12','team 12'], ['move 1','move 1'], ['move 1','switch 2'],
        ['switch 2','move 1']], jawLockRosters, 'bss-jaw-lock'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1'], ['move 2','move 2'],
        ['move 1','move 2'], ['move 1','move 2']], ironBallFlingRosters, 'bss-fling-iron-ball'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1'], ['move 2','move 1']],
        kingsRockRosters, 'bss-kings-rock'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1'], ['move 1','move 2']],
        longReachRosters, 'bss-long-reach'],
    ['bssregmc', [['team 1','team 1'], ['move 2','move 1'], ...Array(4).fill(['move 1','move 1']),
        ['move 3','move 1'], ['move 1','move 1'], ['move 2','move 2'], ['move 1','move 1'],
        ['move 2','move 1']], stockpileRosters, 'bss-stockpile'],
    ['bssregmc', [['team 1','team 12'], ['move 1','move 1'], ['move 2','move 2'], ['move 2','switch 2'],
        ['move 2','move 1']], opportunistRosters, 'bss-opportunist'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1'], ['move 1','move 3'],
        ...Array(4).fill(['move 2','move 4']), ['move 1','move 1'], ['move 2','move 2'],
        ['move 1','move 1']], magnetRiseRosters, 'bss-magnet-rise'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1'], ['move 2','move 2']],
        smackDownFlyRosters, 'bss-smack-down-fly'],
    ['bssregmc', [['team 1','team 12'], ['move 1','move 1'], ['move 1','switch 2'],
        ['move 1','switch 2']], blockRosters, 'bss-block'],
    ['vgc2026regmc', [['team 123','team 12'], ['move 1, move 1','move 1, move 1'],
        ['move 1, switch 3','move 1, move 1']], howlRosters, 'vgc-howl'],
    ['vgc2026regmc', [['team 12','team 12'], ['move 1, move 1','move 1, move 1']],
        teeterDanceRosters, 'vgc-teeter-dance'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1'], ['move 1','move 1']],
        marvelScaleRosters, 'bss-marvel-scale'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1'], ['move 2','move 1']],
        grassPeltRosters, 'bss-grass-pelt'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1'], ['move 1','move 2']],
        quickFeetRosters, 'bss-quick-feet'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1'], ['move 1','move 2']],
        sandSpitRosters, 'bss-sand-spit'],
    ['bssregmc', [['team 1','team 1'], ...Array(6).fill(['move 1','move 1'])], moodyRosters, 'bss-moody'],
    ['bssregmc', [['team 1','team 1'], ...Array(3).fill(['move 1','move 1'])], magmaArmorRosters, 'bss-magma-armor', '1,2,3,13'],
    ['bssregmc', [['team 12','team 1'], ['move 1','move 1'], ['move 1','move 2'],
        ['switch 2',null]], mementoRosters, 'bss-memento'],
    ['bssregmc', [['team 1','team 1'], ...Array(3).fill(['move 1','move 1']),
        ['move 1','move 2'], ['move 2','move 2']], minimizeRosters, 'bss-minimize'],
    ['bssregmc', [['team 1','team 1'], ...Array(2).fill(['move 1','move 1'])],
        hazeTieRosters, 'bss-haze-speed-tie'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1'], ['move 1 mega','move 1']],
        megaSpeedTieRosters, 'bss-mega-speed-stage-tie'],
    ['bssregmc', [['team 1','team 12'], ['move 1','move 1'], ['move 2','switch 2'],
        ['move 1','move 1']], poisonBarbFlingRosters, 'bss-fling-poison-barb'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1'], ['move 2','move 1']],
        berryFlingRosters, 'bss-fling-berry'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1']], rattledRosters, 'bss-rattled'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1'], ['move 1','move 1'], ['move 1','move 2'], ['move 1','move 2']],
        cudChewRosters, 'bss-cud-chew'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1'], ['move 1','move 2'], ['move 1','move 2'], ['move 1','move 2'],
        ['move 1','move 2'], ['move 1','move 2']], residualBerryRosters, 'bss-cud-chew-after-residual'],
    ['bssregmc', [['team 1','team 1'], ['move 1 mega','move 1'], ['move 2','move 1'], ['move 3','move 2']],
        megaSolRosters, 'bss-mega-sol'],
    ['vgc2026regmc', [['team 12','team 12'], ['move 1 1, move 1 1','move 1, move 1'],
        ['move 1 2, move 1 1','move 1, move 1']], assuranceRosters, 'vgc-assurance'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1'], ['move 1','move 2']],
        clearSmogRosters, 'bss-clear-smog'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1'], ['move 2','move 1']],
        infernalParadeRosters, 'bss-infernal-parade'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1']], chargeAfterDropsRosters, 'bss-charge-after-drops'],
    ['bssregmc', [['team 12','team 1'], ['move 1','move 1'], ['switch 2','move 2'], ['move 1','move 2'],
        ['move 1','move 2']], healBellRosters, 'bss-heal-bell'],
    ['vgc2026regmc', [['team 12','team 12'], ['move 2, move 1','move 1 2, move 1'],
        ['move 1, move 1','move 2, move 1']], soundproofHealBellRosters, 'vgc-heal-bell-soundproof'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1'], ['move 1','move 1']], swaggerRosters, 'bss-swagger'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1'], ['move 1','move 1']], magicPowderRosters, 'bss-magic-powder'],
    ['bssregmc', [['team 12','team 1'], ['move 1','move 1'], ['move 2','move 2'], ['move 2','move 3']],
        ingrainRosters, 'bss-ingrain-big-root'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1'], ['move 2','move 1'], ['move 2','move 1']],
        strengthSapRosters, 'bss-strength-sap'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1'], ['move 2','move 2'], ['move 3','move 2'],
        ['move 1','move 2'], ['move 4','move 2'], ['move 3','move 1']], gravityRosters, 'bss-gravity'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 2'], ['move 1','move 1'], ['move 1','move 2']],
        steelRollerRosters, 'bss-steel-roller'],
    ['bssregmc', [['team 1','team 12'], ['move 1','switch 2']], traceSwitchRosters, 'bss-trace-switch'],
    ['bssregmc', [['team 12','team 1'], ['move 1','move 1'], ['switch 2','move 2']],
        intimidateSubstituteRosters, 'bss-intimidate-substitute'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1']], direClawSleepRosters, 'bss-dire-claw-sleep', '481,271,827,14'],
    ['bssregmc', [['team 1','team 12'], ['move 1','move 1'], [null,'switch 2']], fellStingerRosters, 'bss-fell-stinger'],
    ['bssregmc', [['team 12','team 1'], ['move 1','move 1'], ['move 2','move 2'], ['switch 2','move 3'],
        ['switch 2','move 2']], statChangeRosters, 'bss-stat-split-swap'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1'], ['move 2','move 2'], ['move 3','move 2'],
        ['move 1','move 2']], wonderRoomRosters, 'bss-wonder-room'],
    ['bssregmc', [['team 1','team 1'], ['move 1 mega','move 1'], ['move 2','move 1']],
        parentalBondRosters, 'bss-parental-bond'],
    ['vgc2026regmc', [['team 12','team 12'], ['move 1 mega, move 1','move 1, move 1']],
        parentalSpreadRosters, 'vgc-parental-bond-spread'],
    ['bssregmc', [['team 1','team 1'], ['move 1','move 1']], disableFirstMoveRosters, 'bss-disable-before-first-move'],
    ['vgc2026regmc', [['team 12','team 12'], ['move 1, move 1','move 1 2, move 1 2']],
        sweetVeilRosters, 'vgc-sweet-veil'],
    ['bssregmc', [['team 12','team 1'], ['move 1','move 1']], guardDogRosters, 'bss-guard-dog'],
    ['bssregmc', [['team 12','team 1'], ['switch 2','move 1'], ['switch 2','move 1']],
        syrupRosters, 'bss-supersweet-syrup'],
    ['randomdoublesbattle', [['move 3, move 3 1','switch 3, move 3 2'], ['move 2 2, move 2 2','move 1, move 3 1'],
        ['move 3, move 2 1','move 2 2, move 3 2'], ['pass, switch 5','switch 5, pass']],
        ['1361,533,1,19','1361,533,2,19'], 'random-doubles-tailwind-replacements', '31415,9265,35897,19'],
    ['rosterranchrumble', [['team 4,2,7,6','team 2,1,3,5'], ['move 4 1, move 3 2','move 4 1, move 3 1'],
        ['move 1 2, move 3 1','move 1 2, move 4 2'], [null,'switch 4, pass'],
        ['move 3 1, move 4 1','move 1 1, switch 3'], ['move 3 2, move 4 2','move 1 1, move 2 1'],
        ['switch 4, pass',null], ['move 1, move 4 1','move 2, move 2 1'], ['switch 3, pass','switch 3, pass']],
        ['1361,533,1,19','1361,533,2,19'], 'ranch-simultaneous-partial-replacements', '31415,9265,35897,19'],
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
        ...players.map((team, i) => '>player p' + (i + 1) + ' ' + JSON.stringify({name: 'player' + i,
            ...(typeof team === 'string' ? {seed: team} : {team: label === 'bss-packed' ?
                Teams.pack(team).replace('|Blaze|', '|0|').replace('|Torrent|', '||') : team})})),
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
    for (const choices of play) await checkpoint(choices.flatMap((c, i) => !c ? [] : [c.startsWith('>') ? c : '>p' + (i + 1) + ' ' + c]));
    await stream.writeEnd();
    save(label + '.json', {formatid, checkpoints});
}
