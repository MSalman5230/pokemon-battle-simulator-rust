//! Experimental Poke-RL JSON-lines worker. Accepts only the oracle-validated
//! benchmark corpus until native team validation is ported.

use pokemon_battle_simulator::{
    adapter::{FORMAT, Game},
    dex::DEX,
    rng::Seed,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    io::{self, BufRead, Write},
    sync::LazyLock,
};

static FIXTURES: LazyLock<Value> = LazyLock::new(|| {
    let data: Value =
        serde_json::from_str(include_str!("../../benchmarks/verified-bss-corpus.json"))
            .expect("checked-in benchmark teams must deserialize");
    assert_eq!(data["revision"], DEX.revision);
    data
});

#[derive(Default)]
struct Worker {
    games: HashMap<String, Game>,
}

fn encode(out: &mut Vec<u8>, value: impl Serialize) -> Result<(), String> {
    serde_json::to_writer(out, &value).map_err(|e| e.to_string())
}

impl Worker {
    fn dispatch(&mut self, query: &Value, out: &mut Vec<u8>) -> Result<(), String> {
        let op = query["op"].as_str().ok_or("missing operation")?;
        if op == "ping" {
            return encode(
                out,
                json!({"format":FORMAT,"revision":DEX.revision,"backend":"rust","scope":"verified-corpus"}),
            );
        }
        if op == "batch" {
            let requests = query["requests"]
                .as_array()
                .ok_or("batch must contain requests")?;
            out.push(b'[');
            for (index, request) in requests.iter().enumerate() {
                if index != 0 {
                    out.push(b',');
                }
                self.dispatch_one(request, out)?;
            }
            out.push(b']');
            return Ok(());
        }
        self.dispatch_one(query, out)
    }

    fn dispatch_one(&mut self, query: &Value, out: &mut Vec<u8>) -> Result<(), String> {
        match query["op"].as_str() {
            Some("new" | "step" | "close" | "trace") => {}
            Some(op) => return Err(format!("not ported: operation {op}")),
            None => return Err("missing operation".into()),
        }
        let key = query["game"].as_str().ok_or("game must be a string")?;
        match query["op"].as_str().unwrap() {
            "new" => {
                let teams = query["teams"]
                    .as_array()
                    .filter(|t| t.len() == 2)
                    .ok_or("expected two rosters")?;
                for team in teams {
                    if !FIXTURES["teams"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|row| &row["team"] == team)
                    {
                        return Err("not ported: general BSS team validation; expected an exact roster from the verified benchmark corpus".into());
                    }
                }
                let teams = [0, 1].map(|i| teams[i].as_array().unwrap().clone());
                let seed = if let Some(seed) = query["seed"].as_str() {
                    seed.parse::<Seed>().map_err(|e| e.to_string())?
                } else {
                    Seed::Gen5(
                        serde_json::from_value(query["seed"].clone()).map_err(|e| e.to_string())?,
                    )
                };
                let max_turns = query["max_turns"]
                    .as_u64()
                    .and_then(|n| u32::try_from(n).ok())
                    .unwrap_or(200);
                let training = query["training"].as_bool().unwrap_or(false);
                let game = Game::new_prevalidated(teams, seed, max_turns, training)?;
                encode(out, game.result())?;
                self.games.insert(key.into(), game);
                Ok(())
            }
            "step" => {
                let actions: [Option<usize>; 2] =
                    serde_json::from_value(query["actions"].clone()).map_err(|e| e.to_string())?;
                let game = self.games.get_mut(key).ok_or("game does not exist")?;
                game.step(actions)?;
                encode(out, game.result())
            }
            "trace" => encode(
                out,
                self.games.get(key).ok_or("game does not exist")?.trace(),
            ),
            "close" => {
                self.games.remove(key);
                encode(out, true)
            }
            _ => unreachable!(),
        }
    }
}

fn main() -> io::Result<()> {
    let mut worker = Worker::default();
    let mut stdin = io::stdin().lock();
    let mut stdout = io::stdout().lock();
    let mut line = String::new();
    let mut out = Vec::new();
    while stdin.read_line(&mut line)? != 0 {
        out.clear();
        match serde_json::from_str::<Value>(&line) {
            Ok(query) => {
                out.extend_from_slice(b"{\"id\":");
                serde_json::to_writer(&mut out, &query["id"])?;
                out.extend_from_slice(b",\"result\":");
                if let Err(error) = worker.dispatch(&query, &mut out) {
                    out.clear();
                    serde_json::to_writer(&mut out, &json!({"id":query["id"],"error":error}))?;
                } else {
                    out.push(b'}');
                }
            }
            Err(e) => serde_json::to_writer(&mut out, &json!({"error":e.to_string()}))?,
        }
        out.push(b'\n');
        stdout.write_all(&out)?;
        stdout.flush()?;
        line.clear();
    }
    Ok(())
}
