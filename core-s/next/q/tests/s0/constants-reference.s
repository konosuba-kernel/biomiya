.option norvc
.option norelax
.text
.globl entry
entry:
addi x2,x2,-112
li x5,0
sd x5,0(x2)
li x5,2047
sd x5,8(x2)
li x5,2048
sd x5,16(x2)
li x5,4095
sd x5,24(x2)
li x5,32767
sd x5,32(x2)
li x5,32768
sd x5,40(x2)
li x5,268435455
sd x5,48(x2)
li x5,2147483648
sd x5,56(x2)
li x5,4294967295
sd x5,64(x2)
li x5,9223372036854775807
sd x5,72(x2)
li x5,-9223372036854775808
sd x5,80(x2)
li x5,-2048
sd x5,88(x2)
li x5,-1
sd x5,96(x2)
li x10,1
addi x11,x2,0
li x12,104
li x17,64
ecall
li x10,0
li x17,93
ecall
