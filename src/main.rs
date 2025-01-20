use eframe::egui::{self, Color32, RichText};
use humansize::{format_size, BINARY};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const MIN_SIZE_FILTER: u64 = 1024 * 100;

#[derive(Clone)]
struct FileInfo {
    path: PathBuf,
    size: u64,
    is_dir: bool,
    name: String,
}

#[derive(Clone)]
struct CacheEntry {
    file_list: Vec<FileInfo>,
    total_size: u64,
    timestamp: Instant,
}

struct DiskAnalyzer {
    root_path: Option<PathBuf>,
    current_path: Option<PathBuf>,
    file_list: Vec<FileInfo>,
    filtered_list: Vec<FileInfo>,
    scanning: bool,
    search_query: String,
    delete_confirmation: Option<FileInfo>,
    total_size: u64,
    show_details: bool,
    min_size_filter: u64,
    show_all: bool,
    cache: HashMap<PathBuf, CacheEntry>,
    auto_refresh: bool,
    last_refresh: Instant,
    sort_by_size: bool,
    show_hidden: bool,
}

impl Default for DiskAnalyzer {
    fn default() -> Self {
        Self {
            root_path: None,
            current_path: None,
            file_list: Vec::new(),
            filtered_list: Vec::new(),
            scanning: false,
            search_query: String::new(),
            delete_confirmation: None,
            total_size: 0,
            show_details: false,
            min_size_filter: MIN_SIZE_FILTER,
            show_all: false,
            cache: HashMap::new(),
            auto_refresh: false,
            last_refresh: Instant::now(),
            sort_by_size: true,
            show_hidden: false,
        }
    }
}

impl DiskAnalyzer {
    fn calculate_dir_size(path: &Path) -> u64 {
        if let Ok(entries) = fs::read_dir(path) {
            entries
                .filter_map(Result::ok)
                .map(|entry| {
                    let path = entry.path();
                    if let Ok(metadata) = entry.metadata() {
                        if metadata.is_file() {
                            metadata.len()
                        } else {
                            Self::calculate_dir_size(&path)
                        }
                    } else {
                        0
                    }
                })
                .sum()
        } else {
            0
        }
    }

    fn scan_current_directory(&mut self) {
        let current_path = match &self.current_path {
            Some(path) => path.clone(),
            None => return,
        };

        self.scanning = true;
        self.file_list.clear();

        if let Some(cache_entry) = self.cache.get(&current_path) {
            if cache_entry.timestamp.elapsed() < Duration::from_secs(300) {
                self.file_list = cache_entry.file_list.clone();
                self.total_size = cache_entry.total_size;
                self.sort_files();
                self.update_search();
                self.scanning = false;
                return;
            }
        }

        if let Ok(entries) = fs::read_dir(&current_path) {
            let mut files = Vec::new();
            for entry in entries.filter_map(Result::ok) {
                let path = entry.path();
                if let Ok(metadata) = entry.metadata() {
                    let size = if metadata.is_file() {
                        metadata.len()
                    } else {
                        Self::calculate_dir_size(&path)
                    };

                    let name = path.file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();

                    if !self.show_hidden && name.starts_with('.') {
                        continue;
                    }

                    if !self.show_all && size < self.min_size_filter {
                        continue;
                    }

                    files.push(FileInfo {
                        path,
                        size,
                        is_dir: metadata.is_dir(),
                        name,
                    });
                }
            }

            self.file_list = files;
            self.sort_files();
            self.total_size = self.file_list.iter()
                .map(|f| f.size)
                .sum();

            self.cache.insert(current_path, CacheEntry {
                file_list: self.file_list.clone(),
                total_size: self.total_size,
                timestamp: Instant::now(),
            });
        }

        self.update_search();
        self.scanning = false;
    }

    fn sort_files(&mut self) {
        if self.sort_by_size {
            self.file_list.sort_by(|a, b| {
                if a.is_dir == b.is_dir {
                    b.size.cmp(&a.size)
                } else {
                    b.is_dir.cmp(&a.is_dir)
                }
            });
        } else {
            self.file_list.sort_by(|a, b| {
                if a.is_dir == b.is_dir {
                    a.name.to_lowercase().cmp(&b.name.to_lowercase())
                } else {
                    b.is_dir.cmp(&a.is_dir)
                }
            });
        }
    }

    fn update_search(&mut self) {
        let search_query = self.search_query.to_lowercase();
        self.filtered_list = if search_query.is_empty() {
            self.file_list.clone()
        } else {
            self.file_list
                .iter()
                .filter(|item| {
                    item.name.to_lowercase().contains(&search_query)
                })
                .cloned()
                .collect()
        };
    }

    fn navigate_to(&mut self, path: PathBuf) {
        self.current_path = Some(path);
        self.scan_current_directory();
    }

    fn go_up(&mut self) {
        if let Some(current) = &self.current_path {
            if let Some(parent) = current.parent() {
                if self.root_path.as_ref().map_or(true, |root| parent.starts_with(root)) {
                    self.navigate_to(parent.to_path_buf());
                }
            }
        }
    }

    fn delete_item(&mut self, item: &FileInfo) -> Result<(), String> {
        let path = &item.path;
        if item.is_dir {
            if let Err(e) = fs::remove_dir_all(path) {
                return Err(format!("Error deleting directory: {}", e));
            }
        } else {
            if let Err(e) = fs::remove_file(path) {
                return Err(format!("Error deleting file: {}", e));
            }
        }

        if let Some(current_path) = &self.current_path {
            self.cache.remove(current_path);
        }

        self.file_list.retain(|f| f.path != *path);
        self.update_search();
        
        self.total_size = self.file_list.iter()
            .map(|f| f.size)
            .sum();

        Ok(())
    }

    fn render_path_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("⬆️").clicked() {
                self.go_up();
            }

            if let Some(current) = &self.current_path {
                let mut components: Vec<_> = current.components().collect();
                if let Some(root) = &self.root_path {
                    let root_len = root.components().count();
                    while components.len() > root_len {
                        let path = components.iter().take(components.len()).collect::<PathBuf>();
                        let name = components
                            .last()
                            .and_then(|c| c.as_os_str().to_str())
                            .unwrap_or("");
                        
                        let path_clone = path.clone();
                        if ui.button(name).clicked() {
                            self.navigate_to(path_clone);
                            break;
                        }
                        ui.label(">");
                        components.pop();
                    }
                }
            }
        });
    }

    fn render_file_list(&mut self, ui: &mut egui::Ui) {
        let filtered_list = self.filtered_list.clone();
        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                for item in filtered_list {
                    ui.horizontal(|ui| {
                        let icon = if item.is_dir { "📁" } else { "📄" };
                        let text = RichText::new(format!("{} {} - {}", 
                            icon, 
                            item.name,
                            format_size(item.size, BINARY)
                        )).color(if item.is_dir { Color32::LIGHT_BLUE } else { Color32::WHITE });

                        let item_clone = item.clone();
                        if item.is_dir {
                            if ui.button(text).clicked() {
                                self.navigate_to(item_clone.path.clone());
                            }
                        } else {
                            ui.label(text);
                        }

                        if ui.button("🗑️").clicked() {
                            self.delete_confirmation = Some(item_clone);
                        }

                        if ui.button("ℹ️").clicked() {
                            self.show_details = true;
                        }
                    });
                }
            });
    }
}

impl eframe::App for DiskAnalyzer {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Select Directory").clicked() {
                    if let Some(path) = rfd::FileDialog::new().pick_folder() {
                        self.root_path = Some(path.clone());
                        self.navigate_to(path);
                    }
                }

                if self.current_path.is_some() {
                    if ui.button("🔄").clicked() {
                        self.scan_current_directory();
                    }
                    ui.checkbox(&mut self.auto_refresh, "Auto Refresh");
                    ui.checkbox(&mut self.sort_by_size, "Sort by Size");
                    ui.checkbox(&mut self.show_hidden, "Show Hidden");
                    ui.label(format!("Total Size: {}", format_size(self.total_size, BINARY)));
                }
            });

            ui.horizontal(|ui| {
                ui.label("Search:");
                if ui.text_edit_singleline(&mut self.search_query).changed() {
                    self.update_search();
                }
                
                ui.checkbox(&mut self.show_all, "Show All Files");
                if ui.button("Apply Filter").clicked() {
                    self.scan_current_directory();
                }
            });

            if self.current_path.is_some() {
                self.render_path_bar(ui);
            }
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            if self.scanning {
                ui.spinner();
                ui.heading("Scanning...");
            } else if !self.filtered_list.is_empty() {
                self.render_file_list(ui);
            }
        });

        if let Some(item) = &self.delete_confirmation {
            let item_clone = item.clone();
            egui::Window::new("Confirm Deletion")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(format!(
                        "Are you sure you want to delete {}?",
                        item_clone.name
                    ));
                    ui.horizontal(|ui| {
                        if ui.button("Yes").clicked() {
                            match self.delete_item(&item_clone) {
                                Ok(_) => {
                                    self.delete_confirmation = None;
                                }
                                Err(error) => {
                                    ui.label(RichText::new(error).color(Color32::RED));
                                }
                            }
                        }
                        if ui.button("No").clicked() {
                            self.delete_confirmation = None;
                        }
                    });
                });
        }

        if self.show_details {
            egui::Window::new("File Details")
                .collapsible(true)
                .resizable(true)
                .show(ctx, |ui| {
                    ui.label("Directory Statistics:");
                    ui.label(format!("Total items: {}", self.file_list.len()));
                    ui.label(format!("Total size: {}", format_size(self.total_size, BINARY)));
                    
                    let files_count = self.file_list.iter().filter(|i| !i.is_dir).count();
                    let dirs_count = self.file_list.iter().filter(|i| i.is_dir).count();
                    ui.label(format!("Files: {}", files_count));
                    ui.label(format!("Directories: {}", dirs_count));

                    if ui.button("Close").clicked() {
                        self.show_details = false;
                    }
                });
        }

        if self.auto_refresh && self.last_refresh.elapsed() > Duration::from_secs(30) {
            self.scan_current_directory();
            self.last_refresh = Instant::now();
        }

        if self.scanning {
            ctx.request_repaint();
        }
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 600.0])
            .with_title("Disk Space Analyzer"),
        ..Default::default()
    };
    eframe::run_native(
        "Disk Space Analyzer",
        options,
        Box::new(|_cc| Box::new(DiskAnalyzer::default())),
    )
}
