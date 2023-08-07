# gradescope-to-db

Scrape course data into a database for easier access and analysis

## Usage

Note that the working directory is the workspace root (i.e. the directory containing `.gitignore` and `Cargo.lock`). Paths are relative to that directory.

### Setup

Create the `.env` file by following `example.env`, filling blanks as applicable.

- Set `DATABASE_URL` to the database you want to write to.
  - When you change SQL queries, `sqlx` uses this to check that the query is valid.
  - While the app is running, this is used to connect to the database.
  - By default, the code is configured for SQLite databases, but should be modifiable for other SQL servers.
- Leave `SQLX_OFFLINE` as `true` so `sqlx` doesn't check your database on each compilation.

### Running

```sh
cargo run --bin gradescope-to-db
```

## Development

Install `sqlx-cli` via `cargo install sqlx-cli`

After changing an SQL query, run:

```sh
cargo sqlx prepare --workspace
```

When managing migrations, target the right migrations directory by running the following from the workspace root:

```sh
cargo sqlx migrate <command> --source gradescope-to-db/migrations
```
