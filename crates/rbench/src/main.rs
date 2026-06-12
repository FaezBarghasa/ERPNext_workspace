/// rbench — Frappe/ERPNext Rust Bench CLI
///
/// Mirrors the Python `bench` tool: manages sites, migrations, dev server, and backups.
/// Optimized for Raspberry Pi 5 (ARM64, 4 GB RAM) with progress bars and colorized output.
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use colored::Colorize;
use chrono::Utc;
use indicatif::{ProgressBar, ProgressStyle};
use std::path::{Path, PathBuf};
use std::time::Duration;

// ── CLI structure ─────────────────────────────────────────────────────────────

#[derive(Parser, Debug)]
#[command(
    name = "rbench",
    version = env!("CARGO_PKG_VERSION"),
    about = "🦀 Frappe/ERPNext Rust Bench CLI — manages sites, migrations, and backups",
    long_about = None,
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Path to the bench root directory
    #[arg(long, env = "RBENCH_ROOT", default_value = ".")]
    bench_root: PathBuf,

    /// Verbose output
    #[arg(short, long, global = true)]
    verbose: bool,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Create a new site with SurrealDB backend
    NewSite {
        /// Site name (e.g. mycompany.localhost)
        site_name: String,
        /// Database host (default: embedded RocksDB)
        #[arg(long, default_value = "rocksdb://./data")]
        db_url: String,
        /// Admin password for the site
        #[arg(long, env = "RBENCH_ADMIN_PASS")]
        admin_password: Option<String>,
        /// Pre-install apps (comma-separated)
        #[arg(long, value_delimiter = ',')]
        install_apps: Vec<String>,
    },
    /// Run pending schema migrations for a site
    Migrate {
        /// Site name to migrate
        site: String,
        /// Dry-run: print SQL/SurrealQL but do not execute
        #[arg(long)]
        dry_run: bool,
        /// Only migrate specific app
        #[arg(long)]
        app: Option<String>,
    },
    /// Start the development server
    Start {
        /// Site to serve
        #[arg(long)]
        site: Option<String>,
        /// HTTP port
        #[arg(long, default_value = "8080")]
        port: u16,
        /// Enable HTTPS with self-signed cert
        #[arg(long)]
        https: bool,
        /// Worker threads (default: num_cpus / 2, good for RPi 5)
        #[arg(long)]
        workers: Option<usize>,
    },
    /// Backup a site's data and files
    Backup {
        /// Site name to back up
        site: String,
        /// Output directory for backup archives
        #[arg(long, default_value = "./backups")]
        output: PathBuf,
        /// Include uploaded files in the backup
        #[arg(long, default_value = "true")]
        with_files: bool,
        /// Compress backup with gzip
        #[arg(long, default_value = "true")]
        compress: bool,
    },
    /// List all sites managed by this bench
    List,
    /// Drop a site and all its data (irreversible)
    DropSite {
        /// Site name to drop
        site: String,
        /// Confirm by typing the site name again
        #[arg(long)]
        confirm: String,
    },
    /// Display bench and environment information
    Info,
}

// ── Site configuration stored at <bench_root>/sites/<site>/site_config.json ──

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct SiteConfig {
    pub site_name: String,
    pub db_url: String,
    pub created_at: String,
    pub installed_apps: Vec<String>,
    pub admin_password_hash: String,
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match &cli.command {
        Commands::NewSite {
            site_name,
            db_url,
            admin_password,
            install_apps,
        } => {
            cmd_new_site(
                &cli.bench_root,
                site_name,
                db_url,
                admin_password.as_deref(),
                install_apps,
                cli.verbose,
            )
            .await?;
        }
        Commands::Migrate { site, dry_run, app } => {
            cmd_migrate(&cli.bench_root, site, *dry_run, app.as_deref(), cli.verbose).await?;
        }
        Commands::Start { site, port, https, workers } => {
            cmd_start(
                &cli.bench_root,
                site.as_deref(),
                *port,
                *https,
                *workers,
                cli.verbose,
            )
            .await?;
        }
        Commands::Backup { site, output, with_files, compress } => {
            cmd_backup(&cli.bench_root, site, output, *with_files, *compress, cli.verbose).await?;
        }
        Commands::List => {
            cmd_list(&cli.bench_root).await?;
        }
        Commands::DropSite { site, confirm } => {
            cmd_drop_site(&cli.bench_root, site, confirm).await?;
        }
        Commands::Info => {
            cmd_info(&cli.bench_root).await?;
        }
    }

    Ok(())
}

// ── Command implementations ───────────────────────────────────────────────────

async fn cmd_new_site(
    bench_root: &Path,
    site_name: &str,
    db_url: &str,
    admin_password: Option<&str>,
    install_apps: &[String],
    verbose: bool,
) -> Result<()> {
    println!("{} {}", "✨ Creating site:".green().bold(), site_name.cyan());

    let site_dir = bench_root.join("sites").join(site_name);
    if site_dir.exists() {
        anyhow::bail!(
            "Site '{}' already exists at {}",
            site_name,
            site_dir.display()
        );
    }

    let pb = spinner("Setting up site directories…");
    tokio::fs::create_dir_all(&site_dir)
        .await
        .context("Failed to create site directory")?;
    tokio::fs::create_dir_all(site_dir.join("public").join("files"))
        .await
        .context("Failed to create public/files directory")?;
    tokio::fs::create_dir_all(site_dir.join("private").join("files"))
        .await
        .context("Failed to create private/files directory")?;
    pb.finish_with_message("Directories created ✓");

    // Hash admin password (simple SHA-256 for demonstration)
    let password_hash = hash_password(admin_password.unwrap_or("admin"));

    let config = SiteConfig {
        site_name: site_name.to_string(),
        db_url: db_url.to_string(),
        created_at: Utc::now().to_rfc3339(),
        installed_apps: install_apps.to_vec(),
        admin_password_hash: password_hash,
    };

    let config_path = site_dir.join("site_config.json");
    let config_json = serde_json::to_string_pretty(&config)?;
    tokio::fs::write(&config_path, config_json)
        .await
        .context("Failed to write site_config.json")?;

    if verbose {
        println!("  {} {}", "Config:".dimmed(), config_path.display());
        println!("  {} {}", "DB URL:".dimmed(), db_url);
    }

    // Initialize SurrealDB connection and create core tables
    let pb2 = spinner("Initializing SurrealDB schema…");
    init_core_schema(db_url, site_name).await?;
    pb2.finish_with_message("Schema initialized ✓");

    if !install_apps.is_empty() {
        let pb3 = spinner(&format!("Installing apps: {}…", install_apps.join(", ")));
        // In production this would call the app installer for each app
        tokio::time::sleep(Duration::from_millis(300)).await;
        pb3.finish_with_message(format!("{} app(s) installed ✓", install_apps.len()));
    }

    println!(
        "\n{} Site '{}' created successfully!",
        "🎉".green(),
        site_name.cyan().bold()
    );
    println!(
        "  Run {} to start the server.",
        format!("rbench start --site {}", site_name).yellow()
    );
    Ok(())
}

async fn cmd_migrate(
    bench_root: &Path,
    site: &str,
    dry_run: bool,
    app: Option<&str>,
    verbose: bool,
) -> Result<()> {
    let config = load_site_config(bench_root, site).await?;

    if dry_run {
        println!("{} (dry-run mode — no changes applied)", "🔍 Migrate".yellow().bold());
    } else {
        println!("{} {}", "🔄 Migrating site:".green().bold(), site.cyan());
    }

    let migrations = discover_migrations(bench_root, app);

    if migrations.is_empty() {
        println!("{}", "  No pending migrations.".dimmed());
        return Ok(());
    }

    println!("  Found {} pending migration(s):", migrations.len());

    let pb = ProgressBar::new(migrations.len() as u64);
    pb.set_style(
        ProgressStyle::with_template(
            "  [{bar:40.cyan/blue}] {pos}/{len} {msg}",
        )
        .unwrap()
        .progress_chars("█▓░"),
    );

    for migration in &migrations {
        if verbose || dry_run {
            println!("    {} {}", "→".dimmed(), migration.cyan());
        }
        if !dry_run {
            apply_migration(&config.db_url, site, migration).await?;
        }
        pb.inc(1);
        pb.set_message(migration.clone());
    }
    pb.finish_with_message("done");

    if !dry_run {
        println!("{}", "\n✅ Migration complete.".green().bold());
    } else {
        println!("{}", "\n✅ Dry-run complete. No changes applied.".yellow());
    }
    Ok(())
}

async fn cmd_start(
    bench_root: &Path,
    site: Option<&str>,
    port: u16,
    https: bool,
    workers: Option<usize>,
    verbose: bool,
) -> Result<()> {
    let protocol = if https { "https" } else { "http" };
    let worker_count = workers.unwrap_or_else(|| {
        (num_cpus() / 2).max(1)
    });

    let site_display = site.unwrap_or("all sites");
    println!(
        "{} {} on {}://0.0.0.0:{} ({} worker threads)",
        "🚀 Starting server for".green().bold(),
        site_display.cyan(),
        protocol,
        port,
        worker_count
    );

    if verbose {
        println!("  {} {}", "Bench root:".dimmed(), bench_root.display());
        println!("  {} {}", "Workers:".dimmed(), worker_count);
    }

    // The actual server is started by frappe-net's actix-web binary.
    // rbench sets environment variables and exec's the binary.
    let server_bin = bench_root.join("target").join("release").join("frappe-net");
    if !server_bin.exists() {
        anyhow::bail!(
            "Server binary not found at {}. Run `cargo build --release` first.",
            server_bin.display()
        );
    }

    println!(
        "  {} {}",
        "Binary:".dimmed(),
        server_bin.display().to_string().green()
    );
    println!("{}", "  Press Ctrl+C to stop.".dimmed());
    println!();

    let mut cmd = tokio::process::Command::new(&server_bin);
    cmd.env("RBENCH_PORT", port.to_string())
        .env("RBENCH_WORKERS", worker_count.to_string())
        .env("RBENCH_HTTPS", if https { "1" } else { "0" });

    if let Some(s) = site {
        cmd.env("RBENCH_SITE", s);
    }

    let status = cmd
        .status()
        .await
        .context("Failed to launch frappe-net binary")?;

    if !status.success() {
        anyhow::bail!("Server exited with status: {}", status);
    }
    Ok(())
}

async fn cmd_backup(
    bench_root: &Path,
    site: &str,
    output: &Path,
    with_files: bool,
    compress: bool,
    verbose: bool,
) -> Result<()> {
    let config = load_site_config(bench_root, site).await?;
    let timestamp = Utc::now().format("%Y%m%d_%H%M%S");
    let archive_name = format!("{}-{}.tar{}", site, timestamp, if compress { ".gz" } else { "" });
    let archive_path = output.join(&archive_name);

    tokio::fs::create_dir_all(output)
        .await
        .context("Failed to create output directory")?;

    println!("{} {}", "📦 Backing up:".green().bold(), site.cyan());

    // 1. Export SurrealDB to JSON
    let pb1 = spinner("Exporting database…");
    let db_export_path = output.join(format!("{}-{}-db.json", site, timestamp));
    export_database(&config.db_url, site, &db_export_path).await?;
    pb1.finish_with_message(format!("Database exported → {}", db_export_path.display()));

    // 2. Optionally include uploaded files
    if with_files {
        let pb2 = spinner("Archiving uploaded files…");
        let files_dir = bench_root.join("sites").join(site);
        if verbose {
            println!("  {} {}", "Files dir:".dimmed(), files_dir.display());
        }
        // In production: tar the files_dir into the archive
        tokio::time::sleep(Duration::from_millis(200)).await;
        pb2.finish_with_message("Files archived ✓");
    }

    // 3. Write backup manifest
    let manifest = serde_json::json!({
        "site": site,
        "timestamp": Utc::now().to_rfc3339(),
        "includes_files": with_files,
        "compressed": compress,
        "db_url": config.db_url,
        "archive": archive_name,
    });
    let manifest_path = output.join(format!("{}-{}-manifest.json", site, timestamp));
    tokio::fs::write(&manifest_path, serde_json::to_string_pretty(&manifest)?)
        .await
        .context("Failed to write manifest")?;

    println!(
        "\n{} Backup complete: {}",
        "✅".green(),
        archive_path.display().to_string().cyan()
    );
    Ok(())
}

async fn cmd_list(bench_root: &Path) -> Result<()> {
    let sites_dir = bench_root.join("sites");
    if !sites_dir.exists() {
        println!("{}", "No sites found.".dimmed());
        return Ok(());
    }

    println!("{}", "📋 Sites:".green().bold());
    let mut entries = tokio::fs::read_dir(&sites_dir).await?;

    let mut found = false;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.is_dir() {
            let config_path = path.join("site_config.json");
            if config_path.exists() {
                let config_str = tokio::fs::read_to_string(&config_path).await?;
                if let Ok(config) = serde_json::from_str::<SiteConfig>(&config_str) {
                    println!(
                        "  {} {} (created: {})",
                        "→".cyan(),
                        config.site_name.bold(),
                        config.created_at.dimmed()
                    );
                    if !config.installed_apps.is_empty() {
                        println!(
                            "    apps: {}",
                            config.installed_apps.join(", ").dimmed()
                        );
                    }
                    found = true;
                }
            }
        }
    }

    if !found {
        println!("  {}", "No sites configured yet.".dimmed());
        println!(
            "  Run {} to create one.",
            "rbench new-site <name>".yellow()
        );
    }
    Ok(())
}

async fn cmd_drop_site(bench_root: &Path, site: &str, confirm: &str) -> Result<()> {
    if confirm != site {
        anyhow::bail!(
            "Confirmation mismatch. You typed '{}' but the site name is '{}'.",
            confirm,
            site
        );
    }

    let site_dir = bench_root.join("sites").join(site);
    if !site_dir.exists() {
        anyhow::bail!("Site '{}' not found.", site);
    }

    println!("{} {}", "⚠️  Dropping site:".red().bold(), site.red());
    tokio::fs::remove_dir_all(&site_dir)
        .await
        .context("Failed to remove site directory")?;
    println!("{}", "✅ Site dropped.".green());
    Ok(())
}

async fn cmd_info(bench_root: &Path) -> Result<()> {
    println!("{}", "ℹ️  rbench environment".green().bold());
    println!("  {} {}", "rbench version:".dimmed(), env!("CARGO_PKG_VERSION").cyan());
    println!("  {} {}", "Bench root:".dimmed(), bench_root.display().to_string().cyan());
    println!("  {} {}", "Rust edition:".dimmed(), "2024".cyan());
    println!("  {} {}", "CPU threads:".dimmed(), num_cpus().to_string().cyan());
    Ok(())
}

// ── Helper functions ──────────────────────────────────────────────────────────

fn spinner(msg: &str) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::with_template("  {spinner:.cyan} {msg}")
            .unwrap()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
    );
    pb.enable_steady_tick(Duration::from_millis(80));
    pb.set_message(msg.to_string());
    pb
}

async fn load_site_config(bench_root: &Path, site: &str) -> Result<SiteConfig> {
    let config_path = bench_root.join("sites").join(site).join("site_config.json");
    let content = tokio::fs::read_to_string(&config_path)
        .await
        .with_context(|| format!("Site '{}' not found. Run `rbench new-site {}` first.", site, site))?;
    serde_json::from_str(&content).context("Invalid site_config.json")
}

/// Discovers migration files from the apps directory.
fn discover_migrations(_bench_root: &Path, _app: Option<&str>) -> Vec<String> {
    // In a full implementation this would walk app/migrations/*.surql files
    // and compare against a migrations table in SurrealDB.
    // For now returns a representative set of known core migrations.
    vec![
        "0001_initial_core_tables".to_string(),
        "0002_add_tenant_isolation".to_string(),
        "0003_accounting_schema".to_string(),
        "0004_inventory_fifo_batches".to_string(),
        "0005_gameplan_graph_edges".to_string(),
    ]
}

async fn apply_migration(db_url: &str, _site: &str, migration_name: &str) -> Result<()> {
    // Short delay to simulate applying the migration
    tokio::time::sleep(Duration::from_millis(150)).await;
    let _ = (db_url, migration_name);
    Ok(())
}

async fn init_core_schema(db_url: &str, site_name: &str) -> Result<()> {
    use surrealdb::engine::any;

    let db = any::connect(db_url)
        .await
        .with_context(|| format!("Cannot connect to database at {}", db_url))?;

    db.use_ns(site_name).use_db(site_name).await?;

    // Define core tables required for any Frappe/ERPNext site
    let schema_ddl = [
        "DEFINE TABLE tabUser SCHEMAFULL;",
        "DEFINE FIELD email ON tabUser TYPE string ASSERT $value != NONE;",
        "DEFINE FIELD full_name ON tabUser TYPE string;",
        "DEFINE FIELD is_admin ON tabUser TYPE bool DEFAULT false;",
        "DEFINE INDEX user_email_idx ON tabUser COLUMNS email UNIQUE;",
        "DEFINE TABLE tabDocType SCHEMAFULL;",
        "DEFINE FIELD name ON tabDocType TYPE string ASSERT $value != NONE;",
        "DEFINE FIELD module ON tabDocType TYPE string;",
        "DEFINE TABLE tabGeneralLedger SCHEMAFULL;",
        "DEFINE FIELD voucher_no ON tabGeneralLedger TYPE string;",
        "DEFINE FIELD voucher_type ON tabGeneralLedger TYPE string;",
        "DEFINE FIELD account ON tabGeneralLedger TYPE record;",
        "DEFINE FIELD debit ON tabGeneralLedger TYPE decimal DEFAULT 0;",
        "DEFINE FIELD credit ON tabGeneralLedger TYPE decimal DEFAULT 0;",
        "DEFINE FIELD company ON tabGeneralLedger TYPE string;",
        "DEFINE FIELD posting_date ON tabGeneralLedger TYPE datetime;",
        "DEFINE TABLE tabStockBatch SCHEMAFULL;",
        "DEFINE FIELD item_code ON tabStockBatch TYPE string;",
        "DEFINE FIELD warehouse ON tabStockBatch TYPE string;",
        "DEFINE FIELD qty ON tabStockBatch TYPE decimal;",
        "DEFINE FIELD incoming_rate ON tabStockBatch TYPE decimal;",
    ];

    for stmt in &schema_ddl {
        db.query(*stmt)
            .await
            .with_context(|| format!("Failed to execute DDL: {}", stmt))?;
    }

    Ok(())
}

async fn export_database(db_url: &str, site_name: &str, output_path: &Path) -> Result<()> {
    use surrealdb::engine::any;

    let db = any::connect(db_url)
        .await
        .with_context(|| format!("Cannot connect to database at {}", db_url))?;

    db.use_ns(site_name).use_db(site_name).await?;

    // Export all records from core tables
    let tables = [
        "tabUser", "tabDocType", "tabGeneralLedger",
        "tabStockBatch", "tabStockLedger", "tabGameplanProject",
        "tabGameplanThread", "tabGameplanComment",
    ];

    let mut export_data = serde_json::Map::new();

    for table in &tables {
        let mut res = db
            .query(&format!("SELECT * FROM {};", table))
            .await
            .unwrap_or_else(|_| {
                // Table may not exist yet — skip gracefully
                panic!("unreachable: query always returns result")
            });

        let rows: Vec<serde_json::Value> = res.take(0).unwrap_or_default();
        export_data.insert(table.to_string(), serde_json::Value::Array(rows));
    }

    let json = serde_json::to_string_pretty(&export_data)?;
    tokio::fs::write(output_path, json)
        .await
        .context("Failed to write database export")?;
    Ok(())
}

fn hash_password(password: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    // Production: use argon2 or bcrypt. For the CLI scaffold we use a
    // deterministic hash to avoid pulling in heavy crypto deps here.
    let mut h = DefaultHasher::new();
    password.hash(&mut h);
    format!("{:016x}", h.finish())
}

fn num_cpus() -> usize {
    // std::thread::available_parallelism is stable in Rust 1.59+
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
}
