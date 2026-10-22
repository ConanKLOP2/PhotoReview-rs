use pr_app::startup::launch::ShellLaunch;
use pr_core::abstractions::{FileSystem, Log};

use pr_test_support::{FakeFileSystem, TempRoot};
use std::path::{Path, PathBuf};

struct CwdGuard(PathBuf);
impl Drop for CwdGuard { fn drop(&mut self) { std::env::set_current_dir(&self.0).unwrap(); } }
#[derive(Clone, Copy)] struct NoLog;
impl Log for NoLog { fn enabled(&self)->bool{false} fn info(&self,_:&str){} fn warn(&self,_:&str){} fn error(&self,_:&str,_:Option<&(dyn std::error::Error+'static)>){} }
struct Fs(FakeFileSystem);
impl FileSystem for Fs {
    fn file_exists(&self, p:&Path)->bool { self.0.file_exists(p) }
    fn directory_exists(&self,p:&Path)->bool { self.0.directory_exists(p) }
    fn get_file_stat(&self,p:&Path)->Option<pr_core::abstractions::FileStat>{self.0.get_file_stat(p)}
    fn move_file(&self,a:&Path,b:&Path)->std::io::Result<()>{self.0.move_file(a,b)}
    fn copy_file(&self,a:&Path,b:&Path)->std::io::Result<()>{self.0.copy_file(a,b)}
    fn delete(&self,p:&Path)->std::io::Result<()>{self.0.delete(p)}
    fn open_read_shared(&self,p:&Path,n:usize)->std::io::Result<Box<dyn pr_core::abstractions::ReadSeek>>{self.0.open_read_shared(p,n)}
    fn open_append(&self,p:&Path,d:bool)->std::io::Result<Box<dyn pr_core::abstractions::AppendStream>>{self.0.open_append(p,d)}
    fn write_all_text_atomic(&self,p:&Path,t:&str,d:bool)->std::io::Result<()>{self.0.write_all_text_atomic(p,t,d)}
    fn read_all_text(&self,p:&Path)->std::io::Result<String>{self.0.read_all_text(p)}
    fn enumerate_files(&self,p:&Path,g:&str)->std::io::Result<Vec<PathBuf>>{self.0.enumerate_files(p,g)}
    fn enumerate_directories(&self,p:&Path)->std::io::Result<Vec<PathBuf>>{self.0.enumerate_directories(p)}
    fn create_directory(&self,p:&Path)->std::io::Result<()>{self.0.create_directory(p)}
    fn resolve_real_path(&self,p:&Path)->std::io::Result<PathBuf>{self.0.resolve_real_path(p)}
}
#[test]
// C#: ShellLaunchTests.Parse_RelativeFile_YieldsTheDirectoryOfItsFullPathNotAnEmptyName
fn parse_relative_file_yields_directory_of_full_path() {
    let orig=std::env::current_dir().unwrap(); let _guard=CwdGuard(orig);
    let temp=TempRoot::new("launch-cwd"); std::env::set_current_dir(temp.path()).unwrap();
    let fs=Fs(FakeFileSystem::new());
    fs.0.add_file(PathBuf::from("relative.jpg"), b"x");
    let launch=ShellLaunch::parse(&["relative.jpg"], &fs, &NoLog);
    assert_eq!(launch.launch_folder.as_deref(), Some(std::env::current_dir().unwrap().to_str().unwrap()));
}
