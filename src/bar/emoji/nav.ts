// Arrow keys move through the emoji grid in two dimensions. A section heading takes a
// whole row and isn't a cell, and the next section starts on the row after that. `columns`
// is the grid's column count, so a move lands on the cell the pointer would.

export const EMOJI_COLUMNS = 10;

export interface Cell {
  index: number;
  row: number;
  col: number;
}

export type GridKey = 'left' | 'right' | 'up' | 'down' | 'pageup' | 'pagedown';

/** One cell per item. `counts` is how many items each section has, in order. */
export function place(counts: readonly number[], columns: number): Cell[] {
  const width = Math.max(1, columns);
  const cells: Cell[] = [];
  let row = 0;
  let index = 0;
  for (const count of counts) {
    if (count <= 0) continue;
    row += 1;
    for (let i = 0; i < count; i++) {
      cells.push({ index, row: row + Math.floor(i / width), col: i % width });
      index += 1;
    }
    row += Math.ceil(count / width);
  }
  return cells;
}

function rowsOf(cells: readonly Cell[]): number[] {
  return [...new Set(cells.map((cell) => cell.row))].sort((a, b) => a - b);
}

function onRow(cells: readonly Cell[], row: number): Cell[] {
  return cells.filter((cell) => cell.row === row).sort((a, b) => a.col - b.col);
}

/** The cell at `col`, or the nearest one when this row is shorter. */
function at(cells: readonly Cell[], row: number, col: number): Cell | null {
  const rowCells = onRow(cells, row);
  if (rowCells.length === 0) return null;
  return rowCells.reduce((best, cell) =>
    Math.abs(cell.col - col) < Math.abs(best.col - col) ? cell : best
  );
}

/**
 * The index arrows or paging land on. `pageRows` is how many cell rows fit in the
 * scroller; less than 1 pages by a single row. The ends stay put.
 */
export function move(
  cells: readonly Cell[],
  index: number,
  key: GridKey,
  pageRows: number
): number {
  if (cells.length === 0) return 0;
  const here = cells.find((cell) => cell.index === index) ?? cells[0];
  const rows = rowsOf(cells);
  const rowPos = rows.indexOf(here.row);
  const page = Math.max(1, pageRows);

  if (key === 'left') {
    if (here.col > 0) return at(cells, here.row, here.col - 1)?.index ?? here.index;
    const prev = rows[rowPos - 1];
    if (prev === undefined) return here.index;
    const prevCells = onRow(cells, prev);
    return prevCells[prevCells.length - 1]?.index ?? here.index;
  }
  if (key === 'right') {
    const rowCells = onRow(cells, here.row);
    const maxCol = rowCells[rowCells.length - 1]?.col ?? here.col;
    if (here.col < maxCol) return at(cells, here.row, here.col + 1)?.index ?? here.index;
    const next = rows[rowPos + 1];
    if (next === undefined) return here.index;
    return at(cells, next, 0)?.index ?? here.index;
  }

  const delta = key === 'up' ? -1 : key === 'down' ? 1 : key === 'pageup' ? -page : page;
  const nextRow = rows[Math.max(0, Math.min(rows.length - 1, rowPos + delta))];
  return at(cells, nextRow, here.col)?.index ?? here.index;
}
