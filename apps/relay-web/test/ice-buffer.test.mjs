import test from "node:test";
import assert from "node:assert/strict";

// Mirrors the listen-page ICE hold: candidates that arrive before
// setRemoteDescription must not be dropped.
function holdIce(state, cand) {
  if (state.remoteSet) {
    state.applied.push(cand);
  } else {
    state.held.push(cand);
  }
}

function applyRemote(state) {
  state.remoteSet = true;
  state.applied.push(...state.held.splice(0));
}

test("ice candidates that arrive before the offer is applied are kept", () => {
  const state = { remoteSet: false, held: [], applied: [] };
  holdIce(state, "cand-a");
  holdIce(state, "cand-b");
  assert.deepEqual(state.applied, []);
  assert.equal(state.held.length, 2);
  applyRemote(state);
  holdIce(state, "cand-c");
  assert.deepEqual(state.applied, ["cand-a", "cand-b", "cand-c"]);
  assert.equal(state.held.length, 0);
});
