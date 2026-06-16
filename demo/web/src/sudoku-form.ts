// A 9×9 Sudoku grid input for the sudoku example. Clue cells (the public
// puzzle) are shaded and locked; the rest are editable cells the prover fills
// in (the private solution, pre-filled with a valid answer so the example runs
// green out of the box and can be edited to experiment). "new puzzle" generates
// a fresh randomized grid. `values()` returns the two 81-char grids the
// example's `toInputs` parses — keeping the data contract identical to the
// declarative-field examples.

import type { FormBody } from "./examples";

/// Fisher–Yates shuffle, in place.
function shuffle<T>(a: T[]): T[] {
  for (let i = a.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));
    [a[i], a[j]] = [a[j], a[i]];
  }
  return a;
}

/// A permutation of 0..8 that shuffles the three bands and the three rows
/// within each band — the largest row reordering that preserves Sudoku
/// validity. Used for both rows and columns.
function bandPerm(): number[] {
  const perm: number[] = [];
  for (const band of shuffle([0, 1, 2])) {
    for (const row of shuffle([0, 1, 2])) perm.push(band * 3 + row);
  }
  return perm;
}

/// A random valid completed grid (81 cells, values 1..9). Starts from the
/// canonical shifted-rows pattern, then applies validity-preserving transforms:
/// a digit relabel, band/row and stack/column permutations, and an optional
/// transpose.
function generateSolution(): number[] {
  const base = (r: number, c: number) => (3 * (r % 3) + Math.floor(r / 3) + c) % 9;
  const labels = shuffle([0, 1, 2, 3, 4, 5, 6, 7, 8]);
  const rowOrder = bandPerm();
  const colOrder = bandPerm();
  const transpose = Math.random() < 0.5;

  const out = new Array<number>(81);
  for (let r = 0; r < 9; r++) {
    for (let c = 0; c < 9; c++) {
      const v = labels[base(rowOrder[r], colOrder[c])] + 1;
      out[transpose ? c * 9 + r : r * 9 + c] = v;
    }
  }
  return out;
}

/// Reveal each cell as a clue with ~0.45 probability (≈ 36 clues).
function pickClues(): boolean[] {
  return Array.from({ length: 81 }, () => Math.random() < 0.45);
}

export function sudokuForm(): FormBody {
  const wrap = document.createElement("div");
  wrap.className = "sudoku-wrap";

  const grid = document.createElement("div");
  grid.className = "sudoku";

  let cells: HTMLInputElement[] = [];
  let givens: boolean[] = [];

  const build = (solution: number[], clues: boolean[]) => {
    grid.replaceChildren();
    cells = [];
    givens = clues;

    for (let i = 0; i < 81; i++) {
      const r = Math.floor(i / 9);
      const c = i % 9;

      const cell = document.createElement("input");
      cell.className = "sudoku-cell";
      cell.type = "text";
      cell.inputMode = "numeric";
      cell.maxLength = 1;
      cell.value = String(solution[i]);
      cell.setAttribute("aria-label", `r${r + 1}c${c + 1}`);

      if (clues[i]) {
        cell.readOnly = true;
        cell.tabIndex = -1; // skip clues in keyboard navigation
        cell.classList.add("given");
      } else {
        cell.classList.add("entry");
        cell.addEventListener("input", () => {
          cell.value = cell.value.replace(/[^1-9]/g, "").slice(0, 1);
        });
      }
      // Thicker rules between the 3×3 boxes (and none past the last row/col).
      if (c === 8) cell.classList.add("last-col");
      else if (c % 3 === 2) cell.classList.add("box-right");
      if (r === 8) cell.classList.add("last-row");
      else if (r % 3 === 2) cell.classList.add("box-bottom");

      cells.push(cell);
      grid.append(cell);
    }
  };

  const regenerate = () => build(generateSolution(), pickClues());

  const bar = document.createElement("div");
  bar.className = "sudoku-bar";
  const newBtn = document.createElement("button");
  newBtn.type = "button";
  newBtn.className = "link-btn";
  newBtn.textContent = "↻ new puzzle";
  newBtn.addEventListener("click", regenerate);
  const legend = document.createElement("span");
  legend.className = "sudoku-legend";
  legend.textContent = "shaded = public clues · the rest is your private solution";
  bar.append(newBtn, legend);

  regenerate(); // initial puzzle
  wrap.append(grid, bar);

  const values = () => {
    let puzzle = "";
    let solution = "";
    for (let i = 0; i < 81; i++) {
      puzzle += givens[i] ? cells[i].value : ".";
      solution += cells[i].value === "" ? "." : cells[i].value;
    }
    return { puzzle, solution };
  };

  return { el: wrap, values };
}
