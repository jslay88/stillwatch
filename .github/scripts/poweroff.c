#include <sys/reboot.h>
#include <unistd.h>

int main(void)
{
    sync();
    reboot(RB_POWER_OFF);
    return 1;
}
