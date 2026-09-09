/* Is perf_event_open usable here?
 *
 * The steady-cycle work needs an instruction count. `perf record` is the
 * cheap way to get one and is unavailable on more hosts than expected,
 * for two different reasons that need different responses:
 *
 *   EACCES / EPERM  - a PMU exists but perf_event_paranoid forbids it.
 *                     Fixable by policy; do not escalate on a shared box.
 *   ENOENT          - there is no PMU at all. Common in VMs (measured on
 *                     a Firecracker microVM, 2026-09-08 UTC) and on
 *                     Apple-silicon containers. Nothing to fix; use
 *                     callgrind, whose counts are simulated but exact.
 *
 * Run this before trusting -- or before reporting the absence of -- any
 * hardware counter on a new host:
 *
 *   gcc -O0 -o probe-perf-event-open probe-perf-event-open.c && ./probe-perf-event-open
 *
 * Exit 0 means a counter was opened. Anything else means it was not, and
 * the printed errno says which of the two cases you are in.
 */
#define _GNU_SOURCE
#include <linux/perf_event.h>
#include <sys/syscall.h>
#include <unistd.h>
#include <string.h>
#include <stdio.h>
#include <errno.h>
int main(void){
  struct perf_event_attr a; memset(&a,0,sizeof a);
  a.type=PERF_TYPE_HARDWARE; a.size=sizeof a; a.config=PERF_COUNT_HW_INSTRUCTIONS;
  a.disabled=1; a.exclude_kernel=1; a.exclude_hv=1;
  int fd=syscall(SYS_perf_event_open,&a,0,-1,-1,0);
  if(fd<0){printf("perf_event_open FAILED: %s (errno %d)\n",strerror(errno),errno);return 1;}
  printf("perf_event_open OK fd=%d\n",fd); return 0;
}
