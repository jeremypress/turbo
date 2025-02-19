#![feature(trivial_bounds)]

use std::{
    collections::BTreeMap,
    env::current_dir,
    io::Read,
    time::{Duration, Instant},
};

use anyhow::Result;
use sha2::{Digest, Sha256};
use turbo_tasks::{unit, util::FormatDuration, TurboTasks, UpdateInfo, Vc};
use turbo_tasks_fs::{
    register, DirectoryContent, DirectoryEntry, DiskFileSystem, FileContent, FileSystem,
    FileSystemPath,
};
use turbo_tasks_memory::MemoryBackend;

#[tokio::main]
async fn main() -> Result<()> {
    register();
    include!(concat!(
        env!("OUT_DIR"),
        "/register_example_hash_directory.rs"
    ));

    let tt = TurboTasks::new(MemoryBackend::default());
    let start = Instant::now();

    let task = tt.spawn_root_task(|| {
        Box::pin(async {
            let root = current_dir().unwrap().to_str().unwrap().to_string();
            let disk_fs = DiskFileSystem::new("project".to_string(), root);
            disk_fs.await?.start_watching()?;

            // Smart Pointer cast
            let fs: Vc<Box<dyn FileSystem>> = Vc::upcast(disk_fs);
            let input = fs.root().join("demo".to_string());
            let dir_hash = hash_directory(input);
            print_hash(dir_hash).await?;
            Ok(unit())
        })
    });
    tt.wait_task_completion(task, true).await.unwrap();
    println!("done in {}", FormatDuration(start.elapsed()));

    loop {
        let UpdateInfo {
            duration, tasks, ..
        } = tt
            .get_or_wait_aggregated_update_info(Duration::from_millis(100))
            .await;
        println!("updated {} tasks in {}", tasks, FormatDuration(duration));
    }
}

#[turbo_tasks::function]
async fn print_hash(dir_hash: Vc<String>) -> Result<Vc<()>> {
    println!("DIR HASH: {}", dir_hash.await?.as_str());
    Ok(unit())
}

async fn filename(path: Vc<FileSystemPath>) -> Result<String> {
    Ok(path.await?.path.split('/').last().unwrap().to_string())
}

#[turbo_tasks::function]
async fn hash_directory(directory: Vc<FileSystemPath>) -> Result<Vc<String>> {
    let dir_path = &directory.await?.path;
    let content = directory.read_dir();
    let mut hashes = BTreeMap::new();
    match &*content.await? {
        DirectoryContent::Entries(entries) => {
            for entry in entries.values() {
                match entry {
                    DirectoryEntry::File(path) => {
                        let name = filename(*path).await?;
                        hashes.insert(name, hash_file(*path).await?.clone_value());
                    }
                    DirectoryEntry::Directory(path) => {
                        let name = filename(*path).await?;
                        hashes.insert(name, hash_directory(*path).await?.clone_value());
                    }
                    _ => {}
                }
            }
        }
        DirectoryContent::NotFound => {
            println!("{}: not found", directory.await?.path);
        }
    };
    let hash = hash_content(
        &mut hashes
            .into_values()
            .collect::<Vec<String>>()
            .join(",")
            .as_bytes(),
    );
    println!("hash_directory({})", dir_path);
    Ok(hash)
}

#[turbo_tasks::function]
async fn hash_file(file_path: Vc<FileSystemPath>) -> Result<Vc<String>> {
    let content = file_path.read().await?;
    Ok(match &*content {
        FileContent::Content(file) => hash_content(&mut file.read()),
        FileContent::NotFound => {
            // report error
            Vc::cell("".to_string())
        }
    })
}

fn hash_content<R: Read>(content: &mut R) -> Vc<String> {
    let mut hasher = Sha256::new();
    let mut buf = [0; 1024];
    while let Ok(size) = content.read(&mut buf) {
        hasher.update(&buf[0..size]);
    }
    let result = format!("{:x}", hasher.finalize());

    Vc::cell(result)
}
