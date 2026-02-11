-- VCR Intelligence Tree: Offline-First SQLite Schema
-- Path: internal/db/schema.sql

PRAGMA foreign_keys = ON;

-- Profiles: User identity and aesthetic settings
CREATE TABLE IF NOT EXISTS profiles (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    aesthetic_config TEXT DEFAULT 'brutalist', -- brutalist, editorial, vice
    llm_provider TEXT DEFAULT 'ollama',
    llm_endpoint TEXT DEFAULT 'http://localhost:11434',
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- Context Nodes: "Memories" of music, palettes, or past projects
CREATE TABLE IF NOT EXISTS context_nodes (
    id TEXT PRIMARY KEY,
    profile_id TEXT REFERENCES profiles(id),
    source_app TEXT NOT NULL, -- e.g., 'ColorWizard', 'smúit cairn'
    node_type TEXT NOT NULL,  -- e.g., 'palette', 'beat', 'prompt'
    content TEXT NOT NULL,     -- JSON content of the node
    metadata TEXT,            -- Additional context
    last_accessed DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- Skill Registry: Track installed modular tools
CREATE TABLE IF NOT EXISTS skill_registry (
    skill_slug TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    path TEXT NOT NULL,
    config_json TEXT,
    is_active INTEGER DEFAULT 1
);

-- Activity Log: Persistent history for agentic reasoning
CREATE TABLE IF NOT EXISTS activity_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    profile_id TEXT REFERENCES profiles(id),
    action_type TEXT NOT NULL, -- 'prompt', 'skill_run', 'render'
    input TEXT,
    output TEXT,
    timestamp DATETIME DEFAULT CURRENT_TIMESTAMP
);
