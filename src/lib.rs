#![deny(unsafe_code)]
#![warn(clippy::pedantic)]
#![warn(clippy::nursery)]
#![warn(clippy::std_instead_of_core)]
#![warn(clippy::alloc_instead_of_core)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::module_name_repetitions)]

use cozy_chess::{util::parse_uci_move, Board, Color, File, Piece, Rank, Square};
use search::{EngineToSearch, History, Search, SearchMode, SearchToEngine};
use std::sync::{Arc, Mutex};
use uci::{EngineToUci, Uci, UciToEngine};

mod evaluate;
mod oracle;
mod random_board;
mod search;
mod tt;
mod uci;

pub struct Engine {
    uci: Uci,
    search: Search,
    quit: bool,
    debug: bool,
    options: EngineOptions,
}

impl Engine {
    #[must_use]
    pub fn new() -> Self {
        Self {
            uci: Uci::new(),
            search: Search::new(),
            quit: false,
            debug: false,
            options: EngineOptions::default(),
        }
    }

    #[allow(clippy::too_many_lines)]
    pub fn main_loop(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let (report_tx, report_rx) = crossbeam_channel::unbounded();

        let board = Arc::new(Mutex::new(Board::default()));
        let history = Arc::new(Mutex::new(Vec::new()));

        self.uci.init(report_tx.clone());

        self.search
            .init(report_tx, Arc::clone(&board), Arc::clone(&history));

        println!(
            "{} v{} by {}",
            env!("CARGO_PKG_NAME"),
            env!("CARGO_PKG_VERSION"),
            env!("CARGO_PKG_AUTHORS")
                .split(':')
                .collect::<Vec<_>>()
                .join(", ")
        );

        while !self.quit {
            match report_rx.recv()? {
                EngineReport::Uci(uci_report) => match uci_report {
                    UciToEngine::Uci => self.uci.send(EngineToUci::Identify)?,
                    UciToEngine::Debug(debug) => self.debug = debug,
                    UciToEngine::IsReady => self.uci.send(EngineToUci::Ready)?,
                    UciToEngine::Register => {
                        eprintln!("warning: register uci command not supported");
                    }
                    UciToEngine::Position(new_board, new_history) => {
                        *board.lock().unwrap() = new_board;
                        *history.lock().unwrap() = new_history;
                    }
                    UciToEngine::SetOption { name, value } => {
                        match name.to_ascii_lowercase().as_str() {
                            "hash" => match value {
                                Some(value) => {
                                    let value = value.parse().map_err(|_| {
                                        format!("invalid value for Hash option: {value}")
                                    });

                                    match value {
                                        Ok(value) => {
                                            if let Err(error) = self.options.hash.set(value) {
                                                eprintln!("error: {error}");
                                            }

                                            self.search.send(EngineToSearch::SetHash(
                                                usize::try_from(value)?,
                                            ))?;
                                        }
                                        Err(error) => {
                                            eprintln!("error: {error}");
                                        }
                                    }
                                }
                                None => {
                                    eprintln!("error: missing value for Hash option");
                                }
                            },
                            _ => {
                                eprintln!("warning: unsupported option: {name} = {value:?}");
                            }
                        }
                    }
                    UciToEngine::UciNewGame => {
                        *board.lock().unwrap() = Board::default();
                        *history.lock().unwrap() = Vec::new();

                        self.search.send(EngineToSearch::ClearHash)?;
                    }
                    UciToEngine::Stop => self.search.send(EngineToSearch::Stop)?,
                    UciToEngine::PonderHit => {
                        eprintln!("warning: ponderhit uci command not supported");
                    }
                    UciToEngine::Quit => self.quit()?,
                    UciToEngine::GoInfinite => self
                        .search
                        .send(EngineToSearch::Start(SearchMode::Infinite))?,
                    UciToEngine::GoMoveTime(movetime) => self
                        .search
                        .send(EngineToSearch::Start(SearchMode::MoveTime(movetime)))?,
                    UciToEngine::GoGameTime(gametime) => self
                        .search
                        .send(EngineToSearch::Start(SearchMode::GameTime(gametime)))?,
                    UciToEngine::GoDepth(depth) => self
                        .search
                        .send(EngineToSearch::Start(SearchMode::Depth(depth)))?,

                    UciToEngine::Unknown(error) => {
                        if let Some(error) = error {
                            eprintln!("error: {error}");
                        }
                    }

                    UciToEngine::Eval => {
                        println!("side to move: {}", board.lock().unwrap().side_to_move());
                        println!(
                            "evaluation:   {}",
                            evaluate::evaluate(&board.lock().unwrap())
                        );
                    }
                    UciToEngine::PrintBoard => {
                        let board = board.lock().unwrap();

                        pretty_print_board(&board);

                        println!("{board}");
                        println!("hash: {:x}", board.hash());
                    }
                    UciToEngine::PrintOptions => {
                        println!("Options:");

                        println!(
                            "  {name} = {value}",
                            name = HashOption::name(),
                            value = self.options.hash.get()
                        );
                    }
                    UciToEngine::PlayMove(mv) => {
                        let parsed_move = parse_uci_move(&board.lock().unwrap(), &mv);

                        let mv = match parsed_move {
                            Ok(mv) => mv,
                            Err(err) => {
                                eprintln!("error: {err}");
                                continue;
                            }
                        };

                        let play_result = board.lock().unwrap().try_play(mv);

                        match play_result {
                            Ok(()) => {
                                let board = board.lock().unwrap();

                                history.lock().unwrap().push(History { hash: board.hash() });
                            }
                            Err(err) => {
                                eprintln!("error: {err}");
                            }
                        }
                    }
                    UciToEngine::Help => {
                        println!("Custom commands:");
                        println!("  eval    - evaluate the current position");
                        println!("  board   - display the current board");
                        println!("  options - display the current engine options");
                        println!("  make    - make a move on the board (e.g. make e2e4)");
                        println!("  random  - set the board to a random position");
                        println!("  sleep   - sleep the uci thread for a number of milliseconds (e.g. sleep 1000)");
                    }
                    UciToEngine::RandomPosition => {
                        *board.lock().unwrap() = random_board::random_board();

                        history.lock().unwrap().clear();

                        self.search.send(EngineToSearch::ClearHash)?;

                        println!("board set to random position");
                        println!("{}", board.lock().unwrap());
                        pretty_print_board(&board.lock().unwrap());
                    }
                    UciToEngine::Sleep(ms) => {
                        println!("slept for {ms} ms");
                    }
                },
                EngineReport::Search(search_report) => match search_report {
                    SearchToEngine::BestMove(bestmove) => {
                        self.uci.send(EngineToUci::BestMove(bestmove))?;
                    }
                    search::SearchToEngine::Summary {
                        depth,
                        seldepth,
                        time,
                        cp,
                        nodes,
                        nps,
                        hashfull,
                        pv,
                    } => self.uci.send(EngineToUci::Summary {
                        depth,
                        seldepth,
                        time,
                        cp,
                        nodes,
                        nps,
                        hashfull,
                        pv,
                    })?,
                },
                EngineReport::Error(error) => {
                    eprintln!("error: {error}");
                }
            }
        }

        Ok(())
    }

    fn quit(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.uci.send(EngineToUci::Quit)?;
        self.search.send(EngineToSearch::Quit)?;

        self.quit = true;

        Ok(())
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

pub enum EngineReport {
    Uci(UciToEngine),
    Search(SearchToEngine),
    Error(String),
}

struct EngineOptions {
    hash: HashOption,
}

impl Default for EngineOptions {
    fn default() -> Self {
        Self {
            hash: HashOption(HashOption::default()),
        }
    }
}

trait EngineOption {
    type Value;
    type Error;

    fn name() -> &'static str;
    fn min() -> Self::Value;
    fn max() -> Self::Value;
    fn default() -> Self::Value;

    fn get(&self) -> Self::Value;

    fn set(&mut self, value: Self::Value) -> Result<(), Self::Error>;
}

struct HashOption(pub i64);

impl EngineOption for HashOption {
    type Value = i64;
    type Error = String;

    fn name() -> &'static str {
        "Hash"
    }

    fn min() -> Self::Value {
        1
    }

    fn max() -> Self::Value {
        // what stockfish uses
        i64::MAX / 0x003F_FFFF_FFFF
    }

    fn default() -> Self::Value {
        16
    }

    fn get(&self) -> Self::Value {
        self.0
    }

    fn set(&mut self, value: Self::Value) -> Result<(), Self::Error> {
        if value < Self::min() {
            return Err(format!("{} must be at least {}", Self::name(), Self::min()));
        }

        if value > Self::max() {
            return Err(format!("{} must be at most {}", Self::name(), Self::max()));
        }

        self.0 = value;

        Ok(())
    }
}

fn pretty_print_board(board: &Board) {
    println!("+---+---+---+---+---+---+---+---+");

    for rank in Rank::ALL.into_iter().rev() {
        print!("|");

        for file in File::ALL {
            let square = Square::new(file, rank);

            let piece = board.piece_on(square);

            let colour = board.color_on(square);

            match (piece, colour) {
                (Some(piece), Some(colour)) => {
                    let symbol = match piece {
                        Piece::Pawn => 'p',
                        Piece::Knight => 'n',
                        Piece::Bishop => 'b',
                        Piece::Rook => 'r',
                        Piece::Queen => 'q',
                        Piece::King => 'k',
                    };

                    let symbol = match colour {
                        Color::White => symbol.to_ascii_uppercase(),
                        Color::Black => symbol,
                    };

                    print!(" {symbol} |");
                }
                _ => print!("   |"),
            }
        }

        println!("\n+---+---+---+---+---+---+---+---+");
    }
}