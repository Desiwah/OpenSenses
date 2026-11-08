// SPDX-License-Identifier: GPL-2.0-or-later
/*
 * OpenSenses: lets the OpenSenses service call the firmware methods of
 * Acer Predator / Nitro gaming laptops, the same ones PredatorSense uses
 * on Windows.
 *
 * Two WMI interfaces are exposed, and only their known methods:
 *   AcerGamingFunction  7A4DDFE7-5B5D-40B4-8595-4408E0CC7F56  methods 1-23
 *   APGeAction          61EF69EA-865C-4BC3-A502-A0DEBA0CB531  methods 1-2
 *
 * User space opens /dev/opensenses (root only) and issues
 * OPENSENSES_IOC_CALL with the method and its input bytes; the reply is
 * the method's integer or buffer result. Nothing else is reachable.
 *
 * The in-tree acer-wmi driver calls some of the same methods without
 * binding these devices, so both can be loaded at once.
 */

#include <linux/acpi.h>
#include <linux/fs.h>
#include <linux/miscdevice.h>
#include <linux/module.h>
#include <linux/mutex.h>
#include <linux/slab.h>
#include <linux/uaccess.h>
#include <linux/wmi.h>

#define GAMING_GUID "7A4DDFE7-5B5D-40B4-8595-4408E0CC7F56"
#define APGE_GUID "61EF69EA-865C-4BC3-A502-A0DEBA0CB531"

enum { OS_GAMING = 0, OS_APGE = 1, OS_COUNT };

static const char *const guids[OS_COUNT] = { GAMING_GUID, APGE_GUID };
static const u32 max_method[OS_COUNT] = { 23, 2 };

#define OS_MAX_DATA 32

struct opensenses_call {
	__u32 iface;	/* OS_GAMING or OS_APGE */
	__u32 method;	/* WMI method id */
	__u32 in_len;	/* bytes used in @in */
	__u32 out_type; /* filled in: 0 nothing, 1 integer, 2 buffer */
	__u8 in[OS_MAX_DATA];
	__u64 out_int;
	__u32 out_len;
	__u8 out[OS_MAX_DATA];
	__u32 reserved;
};

#define OPENSENSES_IOC_CALL _IOWR('O', 1, struct opensenses_call)

static DEFINE_MUTEX(call_lock);
static struct wmi_device *bound[OS_COUNT];

static long do_call(struct opensenses_call *c)
{
	struct acpi_buffer in = { c->in_len, c->in };
	struct acpi_buffer out = { ACPI_ALLOCATE_BUFFER, NULL };
	union acpi_object *obj;
	acpi_status status;

	if (c->iface >= OS_COUNT || c->method < 1 || c->method > max_method[c->iface])
		return -EINVAL;
	if (c->in_len > OS_MAX_DATA)
		return -EINVAL;

	mutex_lock(&call_lock);
	if (bound[c->iface])
		status = wmidev_evaluate_method(bound[c->iface], 0, c->method, &in, &out);
	else if (wmi_has_guid(guids[c->iface]))
		/* Another driver owns the device: call it by GUID, as acer-wmi does. */
		status = wmi_evaluate_method(guids[c->iface], 0, c->method, &in, &out);
	else
		status = AE_NOT_EXIST;
	mutex_unlock(&call_lock);

	if (ACPI_FAILURE(status))
		return status == AE_NOT_EXIST ? -ENODEV : -EIO;

	c->out_type = 0;
	c->out_int = 0;
	c->out_len = 0;
	obj = out.pointer;
	if (obj && obj->type == ACPI_TYPE_INTEGER) {
		c->out_type = 1;
		c->out_int = obj->integer.value;
	} else if (obj && obj->type == ACPI_TYPE_BUFFER) {
		c->out_type = 2;
		c->out_len = min_t(u32, obj->buffer.length, OS_MAX_DATA);
		memcpy(c->out, obj->buffer.pointer, c->out_len);
	}
	kfree(out.pointer);
	return 0;
}

static long opensenses_ioctl(struct file *file, unsigned int cmd, unsigned long arg)
{
	struct opensenses_call c;
	long ret;

	if (cmd != OPENSENSES_IOC_CALL)
		return -ENOTTY;
	if (!capable(CAP_SYS_ADMIN))
		return -EPERM;
	if (copy_from_user(&c, (void __user *)arg, sizeof(c)))
		return -EFAULT;
	ret = do_call(&c);
	if (ret)
		return ret;
	if (copy_to_user((void __user *)arg, &c, sizeof(c)))
		return -EFAULT;
	return 0;
}

static const struct file_operations opensenses_fops = {
	.owner = THIS_MODULE,
	.unlocked_ioctl = opensenses_ioctl,
	.compat_ioctl = opensenses_ioctl,
};

static struct miscdevice opensenses_dev = {
	.minor = MISC_DYNAMIC_MINOR,
	.name = "opensenses",
	.fops = &opensenses_fops,
	.mode = 0600,
};

static const int iface_ids[OS_COUNT] = { OS_GAMING, OS_APGE };

static int opensenses_probe(struct wmi_device *wdev, const void *context)
{
	int i = *(const int *)context;

	mutex_lock(&call_lock);
	bound[i] = wdev;
	mutex_unlock(&call_lock);
	dev_info(&wdev->dev, "OpenSenses bound to %s\n", guids[i]);
	return 0;
}

static void opensenses_remove(struct wmi_device *wdev)
{
	int i;

	mutex_lock(&call_lock);
	for (i = 0; i < OS_COUNT; i++)
		if (bound[i] == wdev)
			bound[i] = NULL;
	mutex_unlock(&call_lock);
}

static const struct wmi_device_id opensenses_ids[] = {
	{ .guid_string = GAMING_GUID, .context = &iface_ids[OS_GAMING] },
	{ .guid_string = APGE_GUID, .context = &iface_ids[OS_APGE] },
	{}
};
MODULE_DEVICE_TABLE(wmi, opensenses_ids);

static struct wmi_driver opensenses_driver = {
	.driver = { .name = "opensenses" },
	.id_table = opensenses_ids,
	.probe = opensenses_probe,
	.remove = opensenses_remove,
};

static int __init opensenses_init(void)
{
	int ret;

	if (!wmi_has_guid(GAMING_GUID)) {
		pr_info("opensenses: no Acer gaming firmware interface found\n");
		return -ENODEV;
	}
	ret = wmi_driver_register(&opensenses_driver);
	if (ret)
		return ret;
	ret = misc_register(&opensenses_dev);
	if (ret) {
		wmi_driver_unregister(&opensenses_driver);
		return ret;
	}
	pr_info("opensenses: ready at /dev/opensenses\n");
	return 0;
}

static void __exit opensenses_exit(void)
{
	misc_deregister(&opensenses_dev);
	wmi_driver_unregister(&opensenses_driver);
}

module_init(opensenses_init);
module_exit(opensenses_exit);

MODULE_DESCRIPTION("Firmware access for the OpenSenses Acer gaming laptop app");
MODULE_LICENSE("GPL");
MODULE_VERSION("0.1.0");
