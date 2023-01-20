<template>

</template>

<script lang="ts">
export default {
    methods : {
        get_x_axis() {
            return [0,1,2,3,4,5];
        },
        get_y_axis() {
            return [1,2,3,4];
        },
        get_data() {
            return [
                [0,1,4,3,4,5],
                [1,1,3,3,5,5],
                [2,1,2,3,6,5]
            ]
        },
        get_x_axis_name() {
            return "X axis name"
        },
        get_y_axis_name() {
            return "Y axis name"
        },
        get_z_axis_name() {
            return "Z axis name"
        }
    }
}

</script>