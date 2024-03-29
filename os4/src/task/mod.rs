//! Task management implementation
//!
//! Everything about tas&k management, like starting and switching tasks is
//! implemented here.
//!
//! A single global instance of [`TaskManager`] called `TASK_MANAGER` controls
//! all the tasks in the operating system.
//!
//! Be careful when you see [`__switch`]. Control flow around this function
//! might not be what you expect.

mod context;
mod switch;
#[allow(clippy::module_inception)]
mod task;

use crate::config::MAX_SYSCALL_NUM;
use crate::loader::{get_app_data, get_num_app};
use crate::mm::{VirtAddr, MapPermission, VirtPageNum};
use crate::sync::UPSafeCell;
use crate::trap::TrapContext;
use alloc::vec::Vec;
use lazy_static::*;
pub use switch::__switch;
pub use task::{TaskControlBlock, TaskStatus};

pub use context::TaskContext;

/// The task manager, where all the tasks are managed.
///
/// Functions implemented on `TaskManager` deals with all task state transitions
/// and task context switching. For convenience, you can find wrappers around it
/// in the module level.
///
/// Most of `TaskManager` are hidden behind the field `inner`, to defer
/// borrowing checks to runtime. You can see examples on how to use `inner` in
/// existing functions on `TaskManager`.
pub struct TaskManager {
    /// total number of tasks
    num_app: usize,
    /// use inner value to get mutable access
    inner: UPSafeCell<TaskManagerInner>,
}

/// The task manager inner in 'UPSafeCell'
struct TaskManagerInner {
    /// task list
    tasks: Vec<TaskControlBlock>,
    /// id of current `Running` task
    current_task: usize,
}

lazy_static! {
    /// a `TaskManager` instance through lazy_static!
    pub static ref TASK_MANAGER: TaskManager = {
        info!("init TASK_MANAGER");
        let num_app = get_num_app();
        info!("num_app = {}", num_app);
        let mut tasks: Vec<TaskControlBlock> = Vec::new();
        for i in 0..num_app {
            tasks.push(TaskControlBlock::new(get_app_data(i), i));
        }
        TaskManager {
            num_app,
            inner: unsafe {
                UPSafeCell::new(TaskManagerInner {
                    tasks,
                    current_task: 0,
                })
            },
        }
    };
}
use crate::timer::get_time_us;

impl TaskManager {
    /// Run the first task in task list.
    ///
    /// Generally, the first task in task list is an idle task (we call it zero process later).
    /// But in ch4, we load apps statically, so the first task is a real app.
    fn run_first_task(&self) -> ! {
        let mut inner = self.inner.exclusive_access();
        let next_task = &mut inner.tasks[0];
        next_task.task_status = TaskStatus::Running;

        next_task.stats.first_run_time = get_time_us();

        let next_task_cx_ptr = &next_task.task_cx as *const TaskContext;
        drop(inner);
        let mut _unused = TaskContext::zero_init();
        // before this, we should drop local variables that must be dropped manually
        unsafe {
            __switch(&mut _unused as *mut _, next_task_cx_ptr);
        }
        panic!("unreachable in run_first_task!");
    }

    /// Change the status of current `Running` task into `Ready`.
    fn mark_current_suspended(&self) {
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        inner.tasks[current].task_status = TaskStatus::Ready;
    }

    /// Change the status of current `Running` task into `Exited`.
    fn mark_current_exited(&self) {
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        inner.tasks[current].task_status = TaskStatus::Exited;
    }

    /// Find next task to run and return task id.
    ///
    /// In this case, we only return the first `Ready` task in task list.
    fn find_next_task(&self) -> Option<usize> {
        let inner = self.inner.exclusive_access();
        let current = inner.current_task;
        (current + 1..current + self.num_app + 1)
            .map(|id| id % self.num_app)
            .find(|id| inner.tasks[*id].task_status == TaskStatus::Ready)
    }

    /// Get the current 'Running' task's token.
    fn get_current_token(&self) -> usize {
        let inner = self.inner.exclusive_access();
        inner.tasks[inner.current_task].get_user_token()
    }

    #[allow(clippy::mut_from_ref)]
    /// Get the current 'Running' task's trap contexts.
    fn get_current_trap_cx(&self) -> &mut TrapContext {
        let inner = self.inner.exclusive_access();
        inner.tasks[inner.current_task].get_trap_cx()
    }

    /// Switch current `Running` task to the task we have found,
    /// or there is no `Ready` task and we can exit with all applications completed
    fn run_next_task(&self) {
        if let Some(next) = self.find_next_task() {
            let mut inner = self.inner.exclusive_access();
            let current = inner.current_task;
            inner.tasks[next].task_status = TaskStatus::Running;
            inner.current_task = next;

            inner.tasks[next].stats.first_run_time = get_time_us();

            let current_task_cx_ptr = &mut inner.tasks[current].task_cx as *mut TaskContext;
            let next_task_cx_ptr = &inner.tasks[next].task_cx as *const TaskContext;
            drop(inner);
            // before this, we should drop local variables that must be dropped manually
            unsafe {
                __switch(current_task_cx_ptr, next_task_cx_ptr);
            }
            // go back to user mode
        } else {
            panic!("All applications completed!");
        }
    }

    #[allow(dead_code)]
    // TODO finish sys_tasks_info
    fn get_current_task_info(&self) -> (TaskStatus, [u32; MAX_SYSCALL_NUM], usize) {
        let inner = self.inner.exclusive_access();
        let status = inner.tasks[inner.current_task].task_status;
        let (syscall_record, total_time) = inner.tasks[inner.current_task].stats.get_info();
        (status, syscall_record, total_time)
    }
    
    fn record_syscall(&self, syscall_id: usize){
        let mut inner = self.inner.exclusive_access();
        let current_task = inner.current_task;
        inner.tasks[current_task].stats.system_call_record[syscall_id]+=1;
    }

    #[allow(unused_variables)]
    fn mmap(&self, start: usize, len:usize, port:usize) -> isize {
        let mut inner = self.inner.exclusive_access();
        let current_task = inner.current_task;
        
        let start_va = VirtAddr::from(start).floor();
        let end_va = VirtAddr::from(start+len).ceil();

        for i in start_va.0..end_va.0 {
            // i.into();
            if inner.tasks[current_task].memory_set.is_all_map(crate::mm::VirtPageNum(i)) {
                println!("[task::mod::TaskManager]there is a overlap");
                return -1;
            }
        }

        println!("{}", port);

        // allow user to using this page in User mode 
        let permission = MapPermission::from_bits(((port << 1) | 16) as u8);
       // let a = MapPermission::from_bits((port << 1) as u8);
        inner.tasks[current_task].memory_set.insert_framed_area(VirtAddr(start), VirtAddr(start+len), permission.unwrap());
        

        0
    }

    fn unmap(&self, start: usize, len: usize) -> isize {
        let mut inner = self.inner.exclusive_access();
        let current_task = inner.current_task;

        // println!("start:{}; len:{}", start, len);

        let start_va = VirtAddr::from(start).floor();
        let end_va = VirtAddr::from(start+len).ceil();
        print!("start:{}, end:{}", start_va.0, end_va.0);
        
        for i in start_va.0..end_va.0 {
            if inner.tasks[current_task].memory_set.is_not_all_map(crate::mm::VirtPageNum(i)){
                println!("[TaskManager::umap]:  there is a variable haven't been map");
                return -1;
            }
        }
        
        /*
        1. BC the `MemorySet` is belong to application, so we just using it
        2. for each PTE in this delete area 
            3. for each segment inside the `MemorySet` check if it has been map 
                4. if it has been map for each VPN inside it unmap it 
                5. delete this segment 

        [abandon]
        for unmap action's i think the most important actions it :
        1. get this `MemorySet`
        2. trying to find the `MapArea` that you want to delete.
            3. for each `MapArea` in the `MemorySet`
                4. check it's `VPNRange` to identity that MapArea we want to delete
                5. using it's `unmap` function to unmap all map in this `MapArea` 
        */

        for i in start_va.0..end_va.0 {
            inner.tasks[current_task].memory_set.remove_map_area(VirtPageNum(i));
        }
        // for i in start_va.0..end_va.0 {
        //     if !inner.tasks[current_task].memory_set.unmap(VirtPageNum(i)) {
        //         return -1;
        //     }
        // }
        
        
        
        0
    }

}

/// Run the first task in task list.
pub fn run_first_task() {
    TASK_MANAGER.run_first_task();
}

/// Switch current `Running` task to the task we have found,
/// or there is no `Ready` task and we can exit with all applications completed
fn run_next_task() {
    TASK_MANAGER.run_next_task();
}

/// Change the status of current `Running` task into `Ready`.
fn mark_current_suspended() {
    TASK_MANAGER.mark_current_suspended();
}

/// Change the status of current `Running` task into `Exited`.
fn mark_current_exited() {
    TASK_MANAGER.mark_current_exited();
}

/// Suspend the current 'Running' task and run the next task in task list.
pub fn suspend_current_and_run_next() {
    mark_current_suspended();
    run_next_task();
}

/// Exit the current 'Running' task and run the next task in task list.
pub fn exit_current_and_run_next() {
    mark_current_exited();
    run_next_task();
}

/// Get the current 'Running' task's token.
pub fn current_user_token() -> usize {
    TASK_MANAGER.get_current_token()
}

/// Get the current 'Running' task's trap contexts.
pub fn current_trap_cx() -> &'static mut TrapContext {
    TASK_MANAGER.get_current_trap_cx()
}

// return task information
pub fn get_task_info() -> (TaskStatus, [u32; MAX_SYSCALL_NUM], usize){
    TASK_MANAGER.get_current_task_info()
}

pub fn record_syscall(syscall_id: usize){
    TASK_MANAGER.record_syscall(syscall_id);
}

pub fn mmap(start:usize, len:usize, port:usize) -> isize {
    TASK_MANAGER.mmap(start, len, port)
}

pub fn unmap(start: usize, len:usize) -> isize{
    TASK_MANAGER.unmap(start, len)
}