
use crate::mm::page_table;
use crate::config::{MAX_SYSCALL_NUM, PAGE_SIZE};
use crate::task::{exit_current_and_run_next, suspend_current_and_run_next, TaskStatus, get_task_info, current_user_token, mmap, TASK_MANAGER, unmap};
use crate::timer::get_time_us;

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

#[derive(Clone, Copy)]
pub struct TaskInfo {
    pub status: TaskStatus,
    pub syscall_times: [u32; MAX_SYSCALL_NUM],
    pub time: usize,
}

pub fn sys_exit(exit_code: i32) -> ! {
    info!("[kernel] Application exited with code {}", exit_code);
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    suspend_current_and_run_next();
    0
}

// TODO YOUR JOB: 引入虚地址后重写 sys_get_time
pub fn sys_get_time(_ts: *mut TimeVal, _tz: usize) -> isize {
    let _us = get_time_us();
    let ts = page_table::get_phy_addr(current_user_token(), _ts as usize) as *mut TimeVal;

    unsafe {
        *ts = TimeVal {
            sec: _us / 1_000_000,
            usec: _us % 1_000_000,
        };
    }
    0
    // 能否直接尝试通过，尝试不通过，原因主要在于传入的是VPN+offset需要转化成为PPN+offset
    // unsafe {
    //     *_ts = TimeVal {
    //         sec: _us / 1_000_000,
    //         usec: _us % 1_000_000,
    //     };
    // }
    // 0
}

// CLUE: 从 ch4 开始不再对调度算法进行测试~
pub fn sys_set_priority(_prio: isize) -> isize {
    -1
}

// YOUR JOB:   扩展内核以实现 sys_mmap 和 sys_munmap
#[allow(unused_variables)]
pub fn sys_mmap(start: usize, len: usize, port: usize) -> isize {
    // TODO [start, start+len)中存在已经被映射的页
    // TODO 物理内存不足

    let mut align_len = len;
    if start % PAGE_SIZE != 0 {
        return -1;
    }
    // legality check BC Align by page Thus the lower 12 bit must be 0
    // if ((1<<13)-1) & start > 0 {
    //     print!("return BC start wasn't align by page");
    //     return -1;
    // } 
    // len shouldn't biggest than the maximum size of stack allocater
    // we finish this part in TASK_MANAGER
    if len % PAGE_SIZE != 0 {
        align_len = (len/PAGE_SIZE + 1) * PAGE_SIZE;
    }
    // port: the other part of port should be 0; the port shouldn't be 0
    if port & !0x07 != 0 || port & 0x7 == 0 {
        println!("[syscall::process::sysmmap]port illeglity!");
        return -1;
    }

    // alloacte 

    mmap(start, align_len, port)
}

pub fn sys_munmap(_start: usize, _len: usize) -> isize {
    // BC parameter error just leaf it alone, so we just leaf it alone
    if _start % PAGE_SIZE != 0 || _len % PAGE_SIZE != 0 {
        return -1
    };
    unmap(_start, _len); 


    0
}


#[allow(unused_variables)]
// YOUR JOB: 引入虚地址后重写 sys_task_info
pub fn sys_task_info(ti: *mut TaskInfo) -> isize {
    let (s, st, t) = get_task_info();
    let ts = page_table::get_phy_addr(current_user_token(), ti as usize) as *mut TaskInfo;

    // 1. trying to find the location of the raw pointer.
    // 2. if error which this area havn't been alloc then error
    //      else just give the answer inside it.

    unsafe {
        *ts = TaskInfo{
            status: s,
            syscall_times: st,
            time:t / 1_000,
        }
    }
    0
}
