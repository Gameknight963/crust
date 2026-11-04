#pragma crust begin_c
#include <stdio.h>
#pragma crust end_c

int main()
{
    char text[7] = "hello\n";
    printf(&text[0]);
    return 0;
}
