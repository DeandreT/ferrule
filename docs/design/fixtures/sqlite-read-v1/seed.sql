-- Independently authored tiny fixture; insertion order differs from key order.
CREATE TABLE orders (
    id INTEGER PRIMARY KEY,
    label TEXT COLLATE NOCASE,
    score INTEGER,
    active INTEGER CHECK (active IN (0, 1)),
    ratio REAL
);
INSERT INTO orders VALUES
    (30, 'alpha', 20, 0, 2.5),
    (10, 'alpha', 20, 1, 1.5),
    (40, NULL, NULL, NULL, NULL),
    (20, 'Alpha', 10, 1, 3.0),
    (50, 'x'' OR 1=1 --', 30, 0, 4.0);
