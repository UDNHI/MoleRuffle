// Resolve the app location at runtime so the bundle can be moved freely.
#include <mach-o/dyld.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
int main(void) {
    char executable[PATH_MAX],real[PATH_MAX],script[PATH_MAX];
    uint32_t size=sizeof(executable);
    if(_NSGetExecutablePath(executable,&size) || !realpath(executable,real)) return 1;
    char *slash=strrchr(real,'/'); if(!slash)return 1; *slash=0;
    if(snprintf(script,sizeof(script),"%s/../Resources/launch.sh",real)>=(int)sizeof(script))return 1;
    char *args[]={"/bin/sh",script,NULL};execv(args[0],args);perror("launch");return 1;
}
