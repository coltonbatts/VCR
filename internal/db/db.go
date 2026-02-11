package db

import (
	"database/sql"
	"fmt"
	"os"
	"path/filepath"

	_ "github.com/mattn/go-sqlite3"
)

type DB struct {
	Conn *sql.DB
}

func Open() (*DB, error) {
	home, err := os.UserHomeDir()
	if err != nil {
		return nil, err
	}
	dbPath := filepath.Join(home, ".vcr", "brain.db")

	// Ensure directory exists
	err = os.MkdirAll(filepath.Dir(dbPath), 0755)
	if err != nil {
		return nil, err
	}

	conn, err := sql.Open("sqlite3", dbPath)
	if err != nil {
		return nil, err
	}

	return &DB{Conn: conn}, nil
}

func (db *DB) Init(schemaPath string) error {
	schema, err := os.ReadFile(schemaPath)
	if err != nil {
		return err
	}

	_, err = db.Conn.Exec(string(schema))
	return err
}

func (db *DB) SeedMockData() error {
	// Seed a default profile
	_, err := db.Conn.Exec(`
		INSERT OR IGNORE INTO profiles (id, name, aesthetic_config) 
		VALUES ('default', 'VCR User', 'editorial')
	`)
	if err != nil {
		return err
	}

	// Seed some mock context nodes
	nodes := []struct {
		app     string
		type_   string
		content string
	}{
		{"ColorWizard", "palette", `{"name": "Rick Rubin Gold", "colors": ["#FFD700", "#1A1A1A"]}`},
		{"smúit cairn", "beat", `{"track": "Cairn Logic", "bpm": 128, "mood": "dark"}`},
	}

	for i, n := range nodes {
		id := fmt.Sprintf("node_%d", i)
		_, err = db.Conn.Exec(`
			INSERT OR IGNORE INTO context_nodes (id, profile_id, source_app, node_type, content)
			VALUES (?, 'default', ?, ?, ?)
		`, id, n.app, n.type_, n.content)
		if err != nil {
			return err
		}
	}

	return nil
}
func (db *DB) GetContextNodes() ([]string, error) {
	rows, err := db.Conn.Query("SELECT source_app, node_type, content FROM context_nodes")
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var nodes []string
	for rows.Next() {
		var app, ntype, content string
		if err := rows.Scan(&app, &ntype, &content); err != nil {
			return nil, err
		}
		nodes = append(nodes, fmt.Sprintf("[%s %s] %s", app, ntype, content))
	}
	return nodes, nil
}
