//! Experimental Poke-RL JSON-lines worker.

use pokemon_battle_simulator::{
    adapter::{FORMAT, Game, ObservationDelta, lookahead, tactics},
    dex::DEX,
    rng::Seed,
    validation::TeamValidator,
};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json, value::RawValue};
use std::{
    borrow::Cow,
    collections::HashMap,
    env, fs,
    io::{self, BufRead, Write},
};

struct Worker {
    games: HashMap<String, Box<LiveGame>>,
    verified: Option<Value>,
    observations: Observations,
    pool: Option<rayon::ThreadPool>,
    stream_steps: bool,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Observations {
    #[default]
    Full,
    Compact,
    Delta,
    PackedDelta,
}

struct LiveGame {
    game: Game,
    delta: Option<Box<ObservationDelta>>,
    reply: Vec<u8>,
}

#[derive(Deserialize)]
struct Batch<'a> {
    #[serde(borrow)]
    id: Option<&'a RawValue>,
    op: &'a str,
    #[serde(borrow)]
    requests: Vec<Step<'a>>,
}

#[derive(Deserialize)]
struct Step<'a> {
    op: &'a str,
    #[serde(borrow)]
    game: Cow<'a, str>,
    actions: [Option<usize>; 2],
}

impl Observations {
    fn delta(self) -> bool {
        matches!(self, Self::Delta | Self::PackedDelta)
    }
}

impl Worker {
    fn new() -> Result<Self, String> {
        let mut args = env::args_os().skip(1);
        let mut verified = None;
        let mut observations = Observations::Full;
        let mut pool = None;
        let mut stream_steps = false;
        while let Some(option) = args.next() {
            if option == "--compact-observations" && observations == Observations::Full {
                observations = Observations::Compact;
            } else if option == "--delta-observations" && observations == Observations::Full {
                observations = Observations::Delta;
            } else if option == "--packed-delta-observations" && observations == Observations::Full
            {
                observations = Observations::PackedDelta;
            } else if option == "--parallel-steps" && pool.is_none() {
                let count = args
                    .next()
                    .and_then(|s| s.into_string().ok())
                    .and_then(|s| s.parse::<usize>().ok())
                    .filter(|n| (1..=64).contains(n))
                    .ok_or("--parallel-steps requires a thread count from 1 to 64")?;
                pool = Some(
                    rayon::ThreadPoolBuilder::new()
                        .num_threads(count)
                        .use_current_thread()
                        .build()
                        .map_err(|e| e.to_string())?,
                );
            } else if option == "--stream-steps" && !stream_steps {
                stream_steps = true;
            } else if option == "--verified-teams" && verified.is_none() {
                let path = args.next().ok_or("--verified-teams requires a file")?;
                let data = fs::read(path).map_err(|e| e.to_string())?;
                let manifest: Value = serde_json::from_slice(&data).map_err(|e| e.to_string())?;
                if manifest["revision"] != DEX.revision || manifest["format"] != FORMAT {
                    return Err("verified manifest uses another Showdown revision or format".into());
                }
                if !manifest["teams"].as_array().is_some_and(|rows| {
                    !rows.is_empty()
                        && rows.iter().all(|row| {
                            row["team"].as_array().is_some_and(|t| {
                                t.len() == 6
                                    && t.iter().all(|set| {
                                        set["species"].is_string() && set["moves"].is_array()
                                    })
                            })
                        })
                }) {
                    return Err("invalid verified team manifest".into());
                }
                verified = Some(manifest);
            } else {
                return Err(
                    "usage: pokemon-battle-worker [--verified-teams FILE] [--compact-observations | --delta-observations | --packed-delta-observations] [--parallel-steps N [--stream-steps]]"
                        .into(),
                );
            }
        }
        if stream_steps && pool.is_none() {
            return Err("--stream-steps requires --parallel-steps".into());
        }
        Ok(Self {
            games: HashMap::new(),
            verified,
            observations,
            pool,
            stream_steps,
        })
    }
}

fn encode(out: &mut Vec<u8>, value: impl Serialize) -> Result<(), String> {
    serde_json::to_writer(out, &value).map_err(|e| e.to_string())
}

impl LiveGame {
    fn encode(&mut self, out: &mut Vec<u8>, key: &str, mode: Observations) -> Result<(), String> {
        let result = self.game.result();
        if let Some(delta) = &mut self.delta {
            if mode == Observations::PackedDelta {
                delta.write_packed_json(out, key, &result)
            } else {
                delta.write_json(out, key, &result)
            }
        } else {
            result.write_json(out, mode == Observations::Compact)
        }
        .map_err(|e| e.to_string())
    }
}

impl Worker {
    fn parallel_steps(
        &mut self,
        requests: &[Step<'_>],
        id: &str,
        out: &mut Vec<u8>,
    ) -> Option<Result<(), String>> {
        let pool = self.pool.as_ref()?;
        if requests.len() < 2 {
            return None;
        }
        for (index, request) in requests.iter().enumerate() {
            if request.op != "step"
                || requests[..index]
                    .iter()
                    .any(|seen| seen.game == request.game)
                || !self.games.contains_key(request.game.as_ref())
            {
                return None;
            }
        }
        let mut jobs: Vec<_> = requests
            .iter()
            .map(|request| {
                (
                    request.game.as_ref(),
                    request.actions,
                    self.games.remove_entry(request.game.as_ref()).unwrap(),
                    None,
                )
            })
            .collect();
        let mode = self.observations;
        let stream = self.stream_steps;
        let group_size = if stream {
            jobs.len().div_ceil(pool.current_num_threads())
        } else {
            1
        };
        let prefix = if stream {
            let mut prefix = b"{\"id\":".to_vec();
            prefix.extend_from_slice(id.as_bytes());
            prefix.extend_from_slice(b",\"part\":[");
            prefix
        } else {
            Vec::new()
        };
        let send = pool.install(|| {
            jobs.par_chunks_mut(group_size)
                .enumerate()
                .try_for_each(|(group, jobs)| {
                    for (key, actions, (_, live), error) in jobs.iter_mut() {
                        let mut reply = std::mem::take(&mut live.reply);
                        reply.clear();
                        *error = live
                            .game
                            .step(*actions)
                            .and_then(|()| live.encode(&mut reply, key, mode))
                            .err();
                        live.reply = reply;
                    }
                    if !stream || jobs.iter().any(|(_, _, _, error)| error.is_some()) {
                        return Ok(());
                    }
                    let mut part = prefix.clone();
                    encode(&mut part, group * group_size)?;
                    part.extend_from_slice(b",[");
                    for (index, (_, _, (_, live), _)) in jobs.iter().enumerate() {
                        if index != 0 {
                            part.push(b',');
                        }
                        part.extend_from_slice(&live.reply);
                    }
                    part.extend_from_slice(b"]]}\n");
                    let mut stdout = io::stdout().lock();
                    stdout.write_all(&part).map_err(|e| e.to_string())?;
                    stdout.flush().map_err(|e| e.to_string())
                })
        });
        if !stream {
            out.push(b'[');
        }
        let mut first_error = None;
        for (i, (_, _, (key, live), error)) in jobs.into_iter().enumerate() {
            if !stream {
                if i != 0 {
                    out.push(b',');
                }
                out.extend_from_slice(&live.reply);
            }
            self.games.insert(key, live);
            if first_error.is_none() {
                first_error = error;
            }
        }
        if !stream {
            out.push(b']');
        }
        if let Some(error) = first_error {
            return Some(Err(error));
        }
        Some(send.and_then(|()| {
            if stream {
                encode(out, json!({"$battle_stream":requests.len()}))?;
            }
            Ok(())
        }))
    }

    fn try_parallel_line(&mut self, line: &str, out: &mut Vec<u8>) -> io::Result<bool> {
        if self.pool.is_none() || !line.contains("\"batch\"") {
            return Ok(false);
        }
        let Ok(batch) = serde_json::from_str::<Batch<'_>>(line) else {
            return Ok(false);
        };
        if batch.op != "batch" {
            return Ok(false);
        }
        let id = batch.id.map_or("null", RawValue::get);
        out.extend_from_slice(b"{\"id\":");
        out.extend_from_slice(id.as_bytes());
        out.extend_from_slice(b",\"result\":");
        match self.parallel_steps(&batch.requests, id, out) {
            Some(Ok(())) => out.push(b'}'),
            Some(Err(error)) => {
                self.discard_observations();
                out.clear();
                out.extend_from_slice(b"{\"id\":");
                out.extend_from_slice(id.as_bytes());
                out.extend_from_slice(b",\"error\":");
                serde_json::to_writer(&mut *out, &error)?;
                out.push(b'}');
            }
            None => {
                out.clear();
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn discard_observations(&mut self) {
        if self.observations.delta() {
            for game in self.games.values_mut() {
                if let Some(delta) = &mut game.delta {
                    **delta = ObservationDelta::default();
                }
            }
        }
    }

    fn dispatch(&mut self, query: &Value, out: &mut Vec<u8>) -> Result<(), String> {
        let op = query["op"].as_str().ok_or("missing operation")?;
        if op == "ping" {
            return encode(
                out,
                json!({"format":FORMAT,"revision":DEX.revision,"backend":"rust","scope":
                    if self.verified.is_some() { "verified-corpus" } else { "native-validation" }}),
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
            Some("validate") => return validate(query, out),
            Some("tactics_metadata") => return encode(out, tactics::metadata()),
            Some("calculate") => return encode(out, tactics::calculate_many(&query["queries"])?),
            Some("lookahead") => return encode(out, lookahead::evaluate(query)?),
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
                if let Some(verified) = &self.verified {
                    let allowed = verified["teams"]
                        .as_array()
                        .expect("verified roster array checked at startup");
                    if !teams
                        .iter()
                        .all(|team| allowed.iter().any(|row| &row["team"] == team))
                    {
                        return Err(
                            "expected an exact roster from the loaded verified corpus".into()
                        );
                    }
                }
                let roster = |i: usize| {
                    teams[i]
                        .as_array()
                        .cloned()
                        .ok_or("roster must be an array")
                };
                let teams = [roster(0)?, roster(1)?];
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
                let game = if self.verified.is_some() {
                    Game::new_prevalidated(teams, seed, max_turns, training)?
                } else {
                    Game::new(teams, seed, max_turns, training)?
                };
                let mut live = LiveGame {
                    game,
                    delta: (training && self.observations.delta())
                        .then(Box::<ObservationDelta>::default),
                    reply: Vec::new(),
                };
                live.encode(out, key, self.observations)?;
                self.games.insert(key.into(), Box::new(live));
                Ok(())
            }
            "step" => {
                let actions: [Option<usize>; 2] =
                    Deserialize::deserialize(&query["actions"]).map_err(|e| e.to_string())?;
                let game = self.games.get_mut(key).ok_or("game does not exist")?;
                game.game.step(actions)?;
                game.encode(out, key, self.observations)
            }
            "trace" => encode(
                out,
                self.games
                    .get(key)
                    .ok_or("game does not exist")?
                    .game
                    .trace(),
            ),
            "close" => {
                self.games.remove(key);
                if self.observations.delta() {
                    #[derive(Serialize)]
                    struct Closed<'a> {
                        #[serde(rename = "$battle_closed")]
                        key: &'a str,
                    }
                    encode(out, Closed { key })
                } else {
                    encode(out, true)
                }
            }
            _ => unreachable!(),
        }
    }
}

fn validate(query: &Value, out: &mut Vec<u8>) -> Result<(), String> {
    let validator = TeamValidator::new(FORMAT).map_err(|e| e.to_string())?;
    let results: Result<Vec<_>, String> = if query["teams"].is_null() {
        query["sets"]
            .as_array()
            .ok_or("validate must contain sets or teams")?
            .iter()
            .map(|set| validator.validate_set(set).map_err(|e| e.to_string()))
            .collect()
    } else {
        query["teams"]
            .as_array()
            .ok_or("teams must be an array")?
            .iter()
            .map(|team| {
                let team = team.as_array().ok_or("roster must be an array")?;
                validator.validate_team(team).map_err(|e| e.to_string())
            })
            .collect()
    };
    encode(out, results?)
}

fn main() -> io::Result<()> {
    let mut worker = Worker::new().map_err(io::Error::other)?;
    let mut stdin = io::stdin().lock();
    let mut stdout = io::stdout();
    let mut line = String::new();
    let mut out = Vec::new();
    while stdin.read_line(&mut line)? != 0 {
        out.clear();
        if worker.try_parallel_line(&line, &mut out)? {
            out.push(b'\n');
            stdout.write_all(&out)?;
            stdout.flush()?;
            line.clear();
            continue;
        }
        match serde_json::from_str::<Value>(&line) {
            Ok(query) => {
                out.extend_from_slice(b"{\"id\":");
                serde_json::to_writer(&mut out, &query["id"])?;
                out.extend_from_slice(b",\"result\":");
                if let Err(error) = worker.dispatch(&query, &mut out) {
                    worker.discard_observations();
                    out.clear();
                    serde_json::to_writer(&mut out, &json!({"id":query["id"],"error":error}))?;
                } else {
                    out.push(b'}');
                }
            }
            Err(e) => {
                worker.discard_observations();
                serde_json::to_writer(&mut out, &json!({"error":e.to_string()}))?;
            }
        }
        out.push(b'\n');
        stdout.write_all(&out)?;
        stdout.flush()?;
        line.clear();
    }
    Ok(())
}
