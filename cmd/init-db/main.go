package main

import (
	"fmt"
	"log"

	"github.com/coltonbatts/vcr/tui/internal/db"
)

func main() {
	fmt.Println("Initializing VCR Intelligence Tree...")

	database, err := db.Open()
	if err != nil {
		log.Fatalf("Failed to open DB: %v", err)
	}
	defer database.Conn.Close()

	schemaPath := "internal/db/schema.sql"
	err = database.Init(schemaPath)
	if err != nil {
		log.Fatalf("Failed to init DB: %v", err)
	}

	err = database.SeedMockData()
	if err != nil {
		log.Fatalf("Failed to seed DB: %v", err)
	}

	fmt.Println("VCR Intelligence Tree ready with mock context.")
}
