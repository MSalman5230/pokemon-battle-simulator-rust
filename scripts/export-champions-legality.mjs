#!/usr/bin/env node
// Export rule and learnset data, not validator verdicts or executable handlers.
import assert from 'node:assert/strict';
import {readFileSync, writeFileSync} from 'node:fs';
import {createRequire} from 'node:module';
import {speciesIncludingCosmetics} from './showdown-species.mjs';

const stage = new URL('../.tools/poke-rl-simulator/', import.meta.url);
const require = createRequire(new URL('worker.cjs', stage));
const revision = '29f2d9969159fd23f8bb96e177a70072fe573b0e';
const lock = JSON.parse(readFileSync(new URL('package-lock.json', stage)));
assert(lock.packages['node_modules/pokemon-showdown'].resolved.endsWith('/' + revision));
const {Dex, TeamValidator} = require('pokemon-showdown');

function byID(records) {
    return Object.fromEntries(records.sort(([a], [b]) => a.localeCompare(b, 'en')));
}

function project(dex) {
    const learnsets = new Map();
    const species = byID(speciesIncludingCosmetics(dex).map(s => {
        let parents;
        try {
            parents = dex.species.getFullLearnset(s.id).map(data => {
                const parent = data.species.id;
                if (!learnsets.has(parent)) {
                    learnsets.set(parent, {
                        moves: data.learnset,
                        ...(data.eventData && {events: data.eventData}),
                        ...(data.eventOnly && {eventOnly: true}),
                        ...(data.encounters && {encounters: data.encounters}),
                    });
                }
                return {
                    species: parent,
                    ...(dex.species.eggMovesOnly(data.species, s) && {eggOnly: true}),
                };
            });
        } catch (error) {
            // This is also an explicit missing-data error in checkCanLearn.
            // Preserve that state, rather than treating it as an empty pool.
            if (!String(error).startsWith('Error: Species with no learnset data: ')) throw error;
            parents = null;
        }
        return [s.id, {
            num: s.num, gen: s.gen, tags: s.tags, tier: s.tier, doublesTier: s.doublesTier,
            forme: s.forme, baseForme: s.baseForme, battleOnly: s.battleOnly,
            changesFrom: s.changesFrom, requiredAbility: s.requiredAbility, requiredItems: s.requiredItems,
            requiredMove: s.requiredMove, evoLevel: s.evoLevel, canHatch: s.canHatch, mother: s.mother,
            prevo: s.prevo, nfe: s.nfe, eggGroups: s.eggGroups, parents,
        }];
    }));
    return {species, learnsets: byID([...learnsets])};
}

const champions = project(Dex.mod('champions'));
const mb = project(Dex.mod('championsregmb'));
const regmb = Object.fromEntries(Object.entries(mb).map(([key, rows]) => [key,
    Object.fromEntries(Object.entries(rows).filter(([id, row]) => JSON.stringify(row) !== JSON.stringify(champions[key][id]))),
]));
const formats = byID(Dex.formats.all().filter(f => /^\[Gen 9 Champions\]/.test(f.name)).map(f => {
    const validator = new TeamValidator(f.id);
    const r = validator.ruleTable;
    return [f.id, {
        minLevel: r.minLevel, maxTotalLevel: r.maxTotalLevel, minSourceGen: validator.minSourceGen,
        evLimit: r.evLimit, itemLimit: Number(r.valueRules.get('itemclause') || 0),
        ruleSources: Object.fromEntries(r),
        complexBans: [...r.complexBans.values()], complexTeamBans: [...r.complexTeamBans.values()],
    }];
}));
function lines(rows) {
    return '{\n' + Object.entries(rows).map(([id, row]) => JSON.stringify(id) + ': ' + JSON.stringify(row)).join(',\n') + '\n}';
}
writeFileSync(new URL('../data/champions-legality.json', import.meta.url),
    '{\n"revision": ' + JSON.stringify(revision) + ',\n"formats": ' + lines(formats) +
    ',\n"species": ' + lines(champions.species) + ',\n"learnsets": ' + lines(champions.learnsets) +
    ',\n"regmb": {\n"species": ' + lines(regmb.species) + ',\n"learnsets": ' + lines(regmb.learnsets) + '\n}\n}\n');
