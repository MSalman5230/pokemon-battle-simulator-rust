import assert from 'node:assert/strict';
import test from 'node:test';
import {DeltaDecoder} from './poke_rl_delta.mjs';

const frame = (key, sequence, p1, p2) => ({
    $battle_delta_v1: [
        key, sequence, [p1, p2], [false, false, null],
        ...(sequence === 0 ? [[[{'item': 'Luca berry'}], [{'item': 'Silk scarf'}]]] : []),
    ],
});
const seed = tag => [
    {turn: 1}, [0, {species: tag}], [0, {seen: false}], [0, {move: 'tackle'}],
];
const unchanged = [{}, [], [], null];

test('deltas preserve previous observations and isolate game keys', () => {
    const decoder = new DeltaDecoder();
    const first = decoder.expand(frame('a', 0, seed('A1'), seed('A2')));
    const [second, other] = decoder.expand([
        frame('a', 1, [{turn: 2}, [0, {species: 'A1', hp: 0.5}], [], [1, {move: 'protect'}]], unchanged),
        frame('b', 0, seed('B1'), seed('B2')),
    ]);
    assert.deepEqual(first.views[0].history, [{move: 'tackle'}]);
    assert.deepEqual(first.views[0].own, [{species: 'A1', set: {item: 'Luca berry'}}]);
    assert.deepEqual(second.views[0].history, [{move: 'protect'}]);
    assert.equal(second.views[0].own[0].hp, 0.5);
    assert.strictEqual(second.views[0].own[0].set, first.views[0].own[0].set);
    assert.strictEqual(second.training.critic_views[1].opponent, second.views[0].own);
    assert.equal(other.training.critic_views[1].opponent[0].species, 'B1');
});

test('close, missing sequences, and resets cannot use a stale base', () => {
    const decoder = new DeltaDecoder();
    decoder.expand(frame('slot', 0, seed('first'), seed('foe')));
    assert.throws(() => decoder.expand(frame('slot', 2, unchanged, unchanged)), /Missing observation base/);
    assert.equal(decoder.expand({$battle_closed: 'slot'}), true);
    assert.throws(() => decoder.expand(frame('slot', 1, unchanged, unchanged)), /Missing observation base/);
    decoder.expand(frame('slot', 0, seed('reset'), seed('foe')));
    decoder.clear();
    assert.throws(() => decoder.expand(frame('slot', 1, unchanged, unchanged)), /Missing observation base/);
});
