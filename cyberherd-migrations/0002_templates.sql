CREATE TABLE template_categories (
    owner TEXT NOT NULL REFERENCES owners(owner),
    category TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0,1)),
    PRIMARY KEY (owner, category)
);
-- Tombstones intentionally survive all later seed/import attempts.
CREATE TABLE templates (
    owner TEXT NOT NULL,
    category TEXT NOT NULL,
    variant TEXT NOT NULL,
    text TEXT,
    deleted INTEGER NOT NULL DEFAULT 0 CHECK (deleted IN (0,1)),
    CHECK ((deleted=0 AND text IS NOT NULL) OR (deleted=1 AND text IS NULL)),
    PRIMARY KEY (owner,category,variant),
    FOREIGN KEY (owner,category) REFERENCES template_categories(owner,category)
);
