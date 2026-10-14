use gpui_kit::App;
use gpui_kit::Bounds;
use gpui_kit::Context;
use gpui_kit::FocusHandle;
use gpui_kit::Focusable;
use gpui_kit::Hsla;
use gpui_kit::InteractiveElement as _;
use gpui_kit::IntoElement;
use gpui_kit::KeyDownEvent;
use gpui_kit::MouseButton;
use gpui_kit::MouseDownEvent;
use gpui_kit::ParentElement as _;
use gpui_kit::Pixels;
use gpui_kit::Point;
use gpui_kit::Render;
use gpui_kit::Styled as _;
use gpui_kit::Window;
use gpui_kit::bounds;
use gpui_kit::canvas;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::v_flex;
use gpui_kit::div;
use gpui_kit::fill;
use gpui_kit::point;
use gpui_kit::px;
use gpui_kit::rgb;
use gpui_kit::size;
use std::time::Duration;
use std::time::Instant;

const COLS: usize = 36;
const ROWS: usize = 22;
const CELL: f32 = 14.;
const STRIDE: f32 = 16.;
const BOARD_W: f32 = STRIDE * COLS as f32;
const BOARD_H: f32 = STRIDE * ROWS as f32;
const FOOD: usize = 3;
const TICK_START_MS: u64 = 70;
const TICK_STEP_MS: u64 = 1;
const TICK_FLOOR_MS: u64 = 42;
const DEATH_PAUSE: Duration = Duration::from_millis(500);
/// Frame hitches catch up. A longer gap means the pane was hidden, so the run stays put.
const MAX_CATCHUP: u32 = 4;
const STALL: Duration = Duration::from_millis(280);
const HEAD: u32 = 0xb8f5d6;

pub(super) struct SnakeGame {
    board: Board,
    best: u32,
    focus: FocusHandle,
    /// False until Enter, and again after a tab change.
    playing: bool,
    clock: Option<Instant>,
    dead_at: Option<Instant>,
}

impl SnakeGame {
    pub(super) fn new(cx: &mut Context<Self>) -> Self {
        Self {
            board: Board::fresh(0x005E_ED5A_4EC0_FFEE),
            best: 0,
            focus: cx.focus_handle(),
            playing: false,
            clock: None,
            dead_at: None,
        }
    }

    pub(super) fn pause(&mut self) {
        self.playing = false;
        self.clock = None;
    }

    fn advance(&mut self, now: Instant) {
        if let Some(dead_at) = self.dead_at {
            if now.saturating_duration_since(dead_at) >= DEATH_PAUSE {
                self.board.restart();
                self.dead_at = None;
                self.clock = Some(now);
            }
            return;
        }
        let Some(clock) = self.clock else {
            self.clock = Some(now);
            return;
        };
        let mut elapsed = now.saturating_duration_since(clock);
        if elapsed < self.board.tick() {
            return;
        }
        if elapsed > STALL {
            self.clock = Some(now);
            return;
        }
        let mut steps = 0;
        while elapsed >= self.board.tick() && steps < MAX_CATCHUP {
            elapsed -= self.board.tick();
            steps += 1;
            if self.board.step() == Outcome::Dead {
                self.best = self.best.max(self.board.score);
                self.dead_at = Some(now);
                self.clock = Some(now);
                return;
            }
        }
        if elapsed >= self.board.tick() {
            self.clock = Some(now);
            return;
        }
        self.clock = Some(now.checked_sub(elapsed).unwrap_or(now));
    }

    fn score_label(&self) -> String {
        if self.best > self.board.score {
            format!("{} · best {}", self.board.score, self.best)
        } else {
            self.board.score.to_string()
        }
    }
}

impl Focusable for SnakeGame {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for SnakeGame {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let running = self.playing && window.is_window_active() && !cx.reduce_motion();
        if running {
            self.advance(Instant::now());
            window.request_animation_frame();
        } else {
            self.clock = None;
        }
        let snake = self.board.snake.clone();
        let food = self.board.food.clone();
        let dead = self.board.dead;
        let body = cx.theme().success;
        let food_color = cx.theme().primary;
        let crash = cx.theme().danger;
        let empty = cx.theme().list_hover;
        let head = if dead { crash } else { rgb(HEAD).into() };
        let score = self.score_label();
        let hint = if self.playing {
            "arrows · enter"
        } else {
            "enter"
        };
        let muted = cx.theme().muted_foreground;
        v_flex()
            .id("snake")
            .track_focus(&self.focus)
            .items_center()
            .gap_2()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, window, cx| {
                    this.focus.focus(window, cx);
                }),
            )
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.modifiers.modified() {
                    return;
                }
                if event.keystroke.key == "enter" {
                    if this.playing {
                        this.pause();
                    } else {
                        this.playing = true;
                        this.clock = None;
                    }
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                let dir = match event.keystroke.key.as_str() {
                    "left" | "a" => Some(Dir::Left),
                    "right" | "d" => Some(Dir::Right),
                    "up" | "w" => Some(Dir::Up),
                    "down" | "s" => Some(Dir::Down),
                    _ => None,
                };
                let Some(dir) = dir else {
                    return;
                };
                this.board.turn(dir);
                cx.notify();
                cx.stop_propagation();
            }))
            .child(
                div().w(px(BOARD_W)).h(px(BOARD_H)).flex_none().child(
                    canvas(
                        |_, _, _| {},
                        move |bounds, _, window, _| {
                            paint_board(
                                bounds.origin,
                                &snake,
                                &food,
                                BoardColors {
                                    empty,
                                    body,
                                    head,
                                    food: food_color,
                                },
                                window,
                            );
                        },
                    )
                    .size_full(),
                ),
            )
            .child(div().text_size(px(12.)).text_color(muted).child(score))
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(rgb(0x5c616b))
                    .child(hint),
            )
    }
}

struct BoardColors {
    empty: Hsla,
    body: Hsla,
    head: Hsla,
    food: Hsla,
}

fn paint_board(
    origin: Point<Pixels>,
    snake: &[Cell],
    food: &[Cell],
    colors: BoardColors,
    window: &mut Window,
) {
    let BoardColors {
        empty,
        body,
        head,
        food: food_color,
    } = colors;
    for y in 0..ROWS {
        for x in 0..COLS {
            window.paint_quad(fill(cell_bounds(origin, Cell { x, y }), empty));
        }
    }
    for cell in food {
        window.paint_quad(fill(cell_bounds(origin, *cell), food_color));
    }
    for cell in snake.iter().skip(1) {
        window.paint_quad(fill(cell_bounds(origin, *cell), body));
    }
    if let Some(cell) = snake.first() {
        window.paint_quad(fill(cell_bounds(origin, *cell), head));
    }
}

fn cell_bounds(origin: Point<Pixels>, cell: Cell) -> Bounds<Pixels> {
    bounds(
        point(
            origin.x + px(STRIDE) * cell.x,
            origin.y + px(STRIDE) * cell.y,
        ),
        size(px(CELL), px(CELL)),
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Cell {
    x: usize,
    y: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Dir {
    Left,
    Right,
    Up,
    Down,
}

impl Dir {
    fn opposite(self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Left,
            Self::Up => Self::Down,
            Self::Down => Self::Up,
        }
    }

    fn advance(self, cell: Cell) -> Cell {
        let (x, y) = match self {
            Self::Left => (if cell.x == 0 { COLS - 1 } else { cell.x - 1 }, cell.y),
            Self::Right => ((cell.x + 1) % COLS, cell.y),
            Self::Up => (cell.x, if cell.y == 0 { ROWS - 1 } else { cell.y - 1 }),
            Self::Down => (cell.x, (cell.y + 1) % ROWS),
        };
        Cell { x, y }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Outcome {
    Moved,
    Ate,
    Dead,
}

struct Board {
    snake: Vec<Cell>,
    dir: Dir,
    queued: Option<Dir>,
    food: Vec<Cell>,
    score: u32,
    dead: bool,
    rng: u64,
}

impl Board {
    fn fresh(rng: u64) -> Self {
        let y = ROWS / 2;
        let mut board = Self {
            snake: vec![
                Cell { x: 8, y },
                Cell { x: 7, y },
                Cell { x: 6, y },
                Cell { x: 5, y },
            ],
            dir: Dir::Right,
            queued: None,
            food: Vec::new(),
            score: 0,
            dead: false,
            rng,
        };
        board.fill_food();
        board
    }

    /// Each point shaves one millisecond off the step, down to a floor that stays readable.
    fn tick(&self) -> Duration {
        let shaved = u64::from(self.score).saturating_mul(TICK_STEP_MS);
        Duration::from_millis(TICK_START_MS.saturating_sub(shaved).max(TICK_FLOOR_MS))
    }

    fn restart(&mut self) {
        *self = Self::fresh(self.rng);
    }

    fn head(&self) -> Cell {
        self.snake[0]
    }

    fn turn(&mut self, dir: Dir) {
        if self.dead {
            return;
        }
        // One queued turn. A reverse of that turn, or of the way the snake is
        // already heading, is dropped so a 180 does not end the run.
        let basis = self.queued.unwrap_or(self.dir);
        if dir == basis.opposite() {
            return;
        }
        self.queued = Some(dir);
    }

    fn step(&mut self) -> Outcome {
        if self.dead {
            return Outcome::Dead;
        }
        if let Some(dir) = self.queued.take()
            && dir != self.dir.opposite()
        {
            self.dir = dir;
        }
        let next = self.dir.advance(self.head());
        let tail = *self.snake.last().expect("snake");
        let eating = self.food.contains(&next);
        // The tail leaves its cell unless this bite keeps it, so stepping there is clear.
        let hits = self.snake.contains(&next) && (eating || next != tail);
        if hits {
            self.dead = true;
            return Outcome::Dead;
        }
        self.snake.insert(0, next);
        if !eating {
            self.snake.pop();
            return Outcome::Moved;
        }
        self.food.retain(|cell| *cell != next);
        self.score += 1;
        self.fill_food();
        if self.food.is_empty() {
            self.dead = true;
            return Outcome::Dead;
        }
        Outcome::Ate
    }

    fn fill_food(&mut self) {
        while self.food.len() < FOOD {
            if self.place_food().is_none() {
                break;
            }
        }
    }

    fn place_food(&mut self) -> Option<Cell> {
        let cell = self.pick_open_cell()?;
        self.food.push(cell);
        Some(cell)
    }

    fn pick_open_cell(&mut self) -> Option<Cell> {
        if self.snake.len() + self.food.len() >= COLS * ROWS {
            return None;
        }
        if self.rng == 0 {
            self.rng = 0xA5A5_1234_F00D;
        }
        for _ in 0..COLS * ROWS * 2 {
            let n = xorshift(&mut self.rng);
            let cell = Cell {
                x: (n as usize) % COLS,
                y: ((n >> 32) as usize) % ROWS,
            };
            if self.open(cell) {
                return Some(cell);
            }
        }
        (0..ROWS).find_map(|y| {
            (0..COLS).find_map(|x| {
                let cell = Cell { x, y };
                self.open(cell).then_some(cell)
            })
        })
    }

    fn open(&self, cell: Cell) -> bool {
        !self.snake.contains(&cell) && !self.food.contains(&cell)
    }
}

fn xorshift(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lane(dir: Dir, food: Cell) -> Board {
        Board {
            snake: vec![
                Cell { x: 8, y: 8 },
                Cell { x: 7, y: 8 },
                Cell { x: 6, y: 8 },
                Cell { x: 5, y: 8 },
            ],
            dir,
            queued: None,
            food: vec![food],
            score: 0,
            dead: false,
            rng: 0x1234_5678_9ABC,
        }
    }

    #[test]
    fn starts_clear_of_its_food() {
        let board = Board::fresh(0x005E_ED5A_4EC0_FFEE);
        assert_eq!(board.snake.len(), 4);
        assert_eq!(board.dir, Dir::Right);
        assert_eq!(board.food.len(), FOOD);
        assert!(board.food.iter().all(|cell| !board.snake.contains(cell)));
        assert_ne!(board.food[0], board.food[1]);
        assert_ne!(board.food[0], board.food[2]);
        assert_ne!(board.food[1], board.food[2]);
    }

    #[test]
    fn speeds_up_as_the_score_climbs() {
        let mut board = lane(Dir::Right, Cell { x: 20, y: 2 });
        let start = board.tick();
        board.score = 6;
        assert!(board.tick() < start);
        board.score = 80;
        assert_eq!(board.tick(), Duration::from_millis(TICK_FLOOR_MS));
    }

    #[test]
    fn wraps_across_both_edges() {
        let mut board = Board {
            snake: vec![
                Cell { x: 0, y: 4 },
                Cell { x: 1, y: 4 },
                Cell { x: 2, y: 4 },
                Cell { x: 3, y: 4 },
            ],
            dir: Dir::Left,
            queued: None,
            food: vec![Cell { x: 10, y: 10 }],
            score: 0,
            dead: false,
            rng: 1,
        };
        assert_eq!(board.step(), Outcome::Moved);
        assert_eq!(board.head(), Cell { x: COLS - 1, y: 4 });

        let mut board = Board {
            snake: vec![
                Cell { x: 4, y: 0 },
                Cell { x: 4, y: 1 },
                Cell { x: 4, y: 2 },
                Cell { x: 4, y: 3 },
            ],
            dir: Dir::Up,
            queued: None,
            food: vec![Cell { x: 10, y: 10 }],
            score: 0,
            dead: false,
            rng: 1,
        };
        assert_eq!(board.step(), Outcome::Moved);
        assert_eq!(board.head(), Cell { x: 4, y: ROWS - 1 });
    }

    #[test]
    fn eating_grows_and_moves_the_food() {
        let mut board = lane(Dir::Right, Cell { x: 9, y: 8 });
        assert_eq!(board.step(), Outcome::Ate);
        assert_eq!(board.score, 1);
        assert_eq!(board.snake.len(), 5);
        assert_eq!(board.head(), Cell { x: 9, y: 8 });
        assert!(!board.food.contains(&Cell { x: 9, y: 8 }));
        assert!(board.food.iter().all(|cell| !board.snake.contains(cell)));
        assert_eq!(board.food.len(), FOOD);
    }

    #[test]
    fn eating_one_square_leaves_the_others() {
        let mut board = lane(Dir::Right, Cell { x: 9, y: 8 });
        board.food.push(Cell { x: 14, y: 4 });
        board.food.push(Cell { x: 3, y: 12 });
        assert_eq!(board.step(), Outcome::Ate);
        assert!(board.food.contains(&Cell { x: 14, y: 4 }));
        assert!(board.food.contains(&Cell { x: 3, y: 12 }));
        assert!(!board.food.contains(&Cell { x: 9, y: 8 }));
    }

    #[test]
    fn queues_one_turn_and_refuses_the_reverse() {
        let mut board = lane(Dir::Right, Cell { x: 20, y: 2 });
        board.turn(Dir::Left);
        assert_eq!(board.step(), Outcome::Moved);
        assert_eq!(board.dir, Dir::Right);
        assert_eq!(board.head(), Cell { x: 9, y: 8 });

        board.turn(Dir::Up);
        board.turn(Dir::Down);
        assert_eq!(board.step(), Outcome::Moved);
        assert_eq!(board.dir, Dir::Up);
        assert_eq!(board.head(), Cell { x: 9, y: 7 });
    }

    #[test]
    fn moving_into_the_tail_is_clear_until_a_bite_keeps_it() {
        let mut board = Board {
            snake: vec![
                Cell { x: 1, y: 0 },
                Cell { x: 2, y: 0 },
                Cell { x: 2, y: 1 },
                Cell { x: 1, y: 1 },
            ],
            dir: Dir::Down,
            queued: None,
            food: vec![Cell { x: 8, y: 8 }],
            score: 0,
            dead: false,
            rng: 1,
        };
        assert_eq!(board.step(), Outcome::Moved);
        assert_eq!(board.head(), Cell { x: 1, y: 1 });
        assert!(!board.snake[1..].contains(&board.head()));

        let mut board = Board {
            snake: vec![
                Cell { x: 1, y: 0 },
                Cell { x: 2, y: 0 },
                Cell { x: 2, y: 1 },
                Cell { x: 1, y: 1 },
            ],
            dir: Dir::Down,
            queued: None,
            food: vec![Cell { x: 1, y: 1 }],
            score: 3,
            dead: false,
            rng: 1,
        };
        assert_eq!(board.step(), Outcome::Dead);
        assert!(board.dead);
        assert_eq!(board.head(), Cell { x: 1, y: 0 });
    }

    #[test]
    fn hitting_the_body_ends_the_run_and_restart_clears_it() {
        let mut board = Board {
            snake: vec![
                Cell { x: 1, y: 0 },
                Cell { x: 2, y: 0 },
                Cell { x: 2, y: 1 },
                Cell { x: 1, y: 1 },
                Cell { x: 0, y: 1 },
                Cell { x: 0, y: 0 },
            ],
            dir: Dir::Down,
            queued: None,
            food: vec![Cell { x: 10, y: 10 }],
            score: 9,
            dead: false,
            rng: 0x99,
        };
        assert_eq!(board.step(), Outcome::Dead);
        assert!(board.dead);
        assert_eq!(board.score, 9);
        board.turn(Dir::Left);
        assert_eq!(board.step(), Outcome::Dead);
        board.restart();
        assert!(!board.dead);
        assert_eq!(board.score, 0);
        assert_eq!(board.snake.len(), 4);
        assert_eq!(board.dir, Dir::Right);
        assert_eq!(board.head(), Cell { x: 8, y: ROWS / 2 });
        assert_eq!(board.food.len(), FOOD);
        assert!(board.queued.is_none());
    }
}
